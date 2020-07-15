-- Mindec -- Music metadata indexer
-- Copyright 2019 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module Model
  ( ArtistId (..)
  , Album (..)
  , AlbumId (..)
  , Track (..)
  , TrackId (..)
  , SearchArtist (..)
  , SearchAlbum (..)
  , SearchResults (..)
  , SearchTrack (..)
  , coverUrl
  , formatDurationSeconds
  , getAlbums
  , getTracks
  , originalReleaseYear
  , search
  , thumbUrl
  , trackUrl
  ) where

import Prelude

import Affjax as Http
import Affjax.ResponseFormat as Http.ResponseFormat
import Data.Array (sortWith)
import Data.Argonaut.Core (Json)
import Control.Monad.Error.Class (class MonadThrow, throwError)
import Data.Argonaut.Decode (decodeJson, getField) as Json
import Data.Argonaut.Decode.Class (class DecodeJson)
import Data.Either (Either (..))
import Data.String as String
import Effect.Aff (Aff)
import Effect.Exception (Error, error)
import Data.Int (rem)

fatal :: forall m a. MonadThrow Error m => String -> m a
fatal = error >>> throwError

newtype ArtistId = ArtistId String

derive instance artistIdEq :: Eq ArtistId
derive instance artistIdOrd :: Ord ArtistId

newtype AlbumId = AlbumId String

derive instance albumIdEq :: Eq AlbumId
derive instance albumIdOrd :: Ord AlbumId

instance showAlbumId :: Show AlbumId where
  show (AlbumId id) = id

newtype TrackId = TrackId String

derive instance trackIdEq :: Eq TrackId
derive instance trackIdOrd :: Ord TrackId

instance showTrackId :: Show TrackId where
  show (TrackId id) = id

thumbUrl :: AlbumId -> String
thumbUrl (AlbumId id) = "/thumb/" <> id

coverUrl :: AlbumId -> String
coverUrl (AlbumId id) = "/cover/" <> id

trackUrl :: TrackId -> String
trackUrl (TrackId id) = "/track/" <> id <> ".flac"

newtype Album = Album
  { id :: AlbumId
  , title :: String
  , artist :: String
  , sortArtist :: String
  , date :: String
  }

instance decodeJsonAlbum :: DecodeJson Album where
  decodeJson json = do
    obj        <- Json.decodeJson json
    id         <- map AlbumId $ Json.getField obj "id"
    title      <- Json.getField obj "title"
    artist     <- Json.getField obj "artist"
    sortArtist <- Json.getField obj "sort_artist"
    date       <- Json.getField obj "date"
    pure $ Album { id, title, artist, sortArtist, date }

getAlbums :: Aff (Array Album)
getAlbums = do
  response <- Http.get Http.ResponseFormat.json "/albums"
  case response.body of
    Left err -> fatal $ "Failed to retrieve albums: " <> Http.printResponseFormatError err
    Right json -> case Json.decodeJson json of
      Left err -> fatal $ "Failed to parse albums: " <> err
      Right albums -> pure $ sortWith (\(Album a) -> a.date) albums

newtype SearchArtist = SearchArtist
  { id :: ArtistId
  , name :: String
  , albums :: Array AlbumId
  }

newtype SearchAlbum = SearchAlbum
  { id :: AlbumId
  , title :: String
  , artist :: String
  , date :: String
  }

newtype SearchTrack = SearchTrack
  { id :: TrackId
  , title :: String
  , artist :: String
  , album :: String
  , albumId :: AlbumId
  }

newtype SearchResults = SearchResults
  { artists :: Array SearchArtist
  , albums :: Array SearchAlbum
  , tracks :: Array SearchTrack
  }

instance decodeJsonSearchArtist :: DecodeJson SearchArtist where
  decodeJson json = do
    obj     <- Json.decodeJson json
    id      <- map ArtistId $ Json.getField obj "id"
    name    <- Json.getField obj "name"
    albums  <- map (map AlbumId) $ Json.getField obj "albums"
    pure $ SearchArtist { id, name, albums }

instance decodeJsonSearchAlbum :: DecodeJson SearchAlbum where
  decodeJson json = do
    obj        <- Json.decodeJson json
    id         <- map AlbumId $ Json.getField obj "id"
    title      <- Json.getField obj "title"
    artist     <- Json.getField obj "artist"
    date       <- Json.getField obj "date"
    pure $ SearchAlbum { id, title, artist, date }

instance decodeJsonSearchTrack :: DecodeJson SearchTrack where
  decodeJson json = do
    obj        <- Json.decodeJson json
    id         <- map TrackId $ Json.getField obj "id"
    title      <- Json.getField obj "title"
    artist     <- Json.getField obj "artist"
    album      <- Json.getField obj "album"
    albumId    <- map AlbumId $ Json.getField obj "album_id"
    pure $ SearchTrack { id, title, artist, album, albumId }

instance decodeJsonSearchResults :: DecodeJson SearchResults where
  decodeJson json = do
    obj     <- Json.decodeJson json
    artists <- Json.getField obj "artists"
    albums  <- Json.getField obj "albums"
    tracks  <- Json.getField obj "tracks"
    pure $ SearchResults { artists, albums, tracks }

search :: String -> Aff SearchResults
search query = do
  response <- Http.get Http.ResponseFormat.json ("/search?q=" <> query)
  case response.body of
    Left err -> fatal $ "Search failed: " <> Http.printResponseFormatError err
    Right json -> case Json.decodeJson json of
      Left err -> fatal $ "Failed to parse search results: " <> err
      Right results -> pure results

newtype Track = Track
  { id :: TrackId
  , discNumber :: Int
  , trackNumber :: Int
  , title :: String
  , artist :: String
  , durationSeconds :: Int
  }

instance decodeJsonTrack :: DecodeJson Track where
  decodeJson json = do
    obj             <- Json.decodeJson json
    id              <- map TrackId $ Json.getField obj "id"
    discNumber      <- Json.getField obj "disc_number"
    trackNumber     <- Json.getField obj "track_number"
    title           <- Json.getField obj "title"
    artist          <- Json.getField obj "artist"
    durationSeconds <- Json.getField obj "duration_seconds"
    pure $ Track { id, discNumber, trackNumber, title, artist, durationSeconds }

decodeAlbumTracks :: Json -> Either String (Array Track)
decodeAlbumTracks json = do
  obj <- Json.decodeJson json
  Json.getField obj "tracks"

getTracks :: AlbumId -> Aff (Array Track)
getTracks (AlbumId aid) = do
  response <- Http.get Http.ResponseFormat.json $ "/album/" <> aid
  case response.body of
    Left err -> fatal $ "Failed to retrieve tracks: " <> Http.printResponseFormatError err
    Right json -> case decodeAlbumTracks json of
      Left err -> fatal $ "Failed to parse tracks: " <> err
      Right tracks -> pure tracks

-- Format a duration of a track in HH:MM:SS format.
-- Examples:
--    7 ->    0:07
--   23 ->    0:23
--   61 ->    1:01
-- 3607 -> 1:00:07
formatDurationSeconds :: Int -> String
formatDurationSeconds dtSeconds =
  let
    seconds    = rem dtSeconds 60
    dtMinutes  = div dtSeconds 60
    minutes    = rem dtMinutes 60
    dtHours    = div dtMinutes 60
    hours      = dtHours
    show2 x    = if x < 10 then "0" <> show x else show x
  in
    if dtHours > 0
      then show hours <> ":" <> show2 minutes <> ":" <> show2 seconds
      else                      show  minutes <> ":" <> show2 seconds

originalReleaseYear :: Album -> String
originalReleaseYear (Album album) = String.take 4 album.date

