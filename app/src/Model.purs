-- Mindec -- Music metadata indexer
-- Copyright 2019 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module Model
  ( Album (..)
  , AlbumId (..)
  , Track (..)
  , TrackId (..)
  , getAlbums
  , getTracks
  , thumbUrl
  , coverUrl
  , trackUrl
  , formatDurationSeconds
  ) where

import Prelude

import Affjax as Http
import Affjax.ResponseFormat as Http.ResponseFormat
import Data.Array (sortWith)
import Data.Argonaut.Core (Json)
import Data.Argonaut.Decode (decodeJson, getField) as Json
import Data.Argonaut.Decode.Class (class DecodeJson)
import Data.Either (Either (..))
import Data.Newtype (class Newtype)
import Effect.Aff (Aff)
import Effect.Exception (Error, error)
import Control.Monad.Error.Class (class MonadThrow, throwError)
import Data.Int (quot, rem)

fatal :: forall m a. MonadThrow Error m => String -> m a
fatal = error >>> throwError

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

coverUrl :: TrackId -> String
coverUrl (TrackId id) = "/cover/" <> id

trackUrl :: TrackId -> String
trackUrl (TrackId id) = "/track/" <> id <> ".flac"

newtype Album = Album
  { id :: AlbumId
  , title :: String
  , artist :: String
  , sortArtist :: String
  , date :: String
  }

derive instance newtypeAlbum :: Newtype Album _

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

newtype Track = Track
  { id :: TrackId
  , discNumber :: Int
  , trackNumber :: Int
  , title :: String
  , artist :: String
  , durationSeconds :: Int
  }

derive instance newtypeTrack :: Newtype Track _

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
