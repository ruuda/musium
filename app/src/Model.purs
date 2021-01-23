-- Musium -- Music playback daemon with web-based library browser
-- Copyright 2019 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module Model
  ( Artist (..)
  , ArtistId (..)
  , Album (..)
  , AlbumId (..)
  , Decibel (..)
  , Track (..)
  , TrackId (..)
  , SearchArtist (..)
  , SearchAlbum (..)
  , SearchResults (..)
  , SearchTrack (..)
  , QueueId (..)
  , QueuedTrack (..)
  , Volume (..)
  , VolumeChange (..)
  , coverUrl
  , changeVolume
  , enqueueTrack
  , formatDurationSeconds
  , getAlbums
  , getArtist
  , getString
  , getQueue
  , getTracks
  , getVolume
  , originalReleaseYear
  , search
  , thumbUrl
  , trackUrl
  ) where

import Prelude

import Affjax as Http
import Affjax.ResponseFormat as Http.ResponseFormat
import Affjax.StatusCode (StatusCode (..))
import Control.Monad.Error.Class (class MonadThrow, throwError)
import Data.Argonaut.Core (Json)
import Data.Argonaut.Decode (decodeJson, getField) as Json
import Data.Argonaut.Decode.Class (class DecodeJson)
import Data.Array (sortWith)
import Data.Either (Either (..))
import Data.Int (rem)
import Data.Maybe (Maybe (Nothing))
import Data.String as String
import Effect.Aff (Aff)
import Effect.Class (liftEffect)
import Effect.Class.Console as Console
import Effect.Exception (Error, error)
import Time as Time
import Time (Instant)

fatal :: forall m a. MonadThrow Error m => String -> m a
fatal = error >>> throwError

newtype ArtistId = ArtistId String

derive instance artistIdEq :: Eq ArtistId
derive instance artistIdOrd :: Ord ArtistId

instance showArtistId :: Show ArtistId where
  show (ArtistId id) = id

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

newtype QueueId = QueueId String

derive instance queueIdEq :: Eq QueueId
derive instance queueIdOrd :: Ord QueueId

instance showQueueId :: Show QueueId where
  show (QueueId id) = id

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
  , artistId :: String
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
    artistId   <- Json.getField obj "artist_id"
    date       <- Json.getField obj "date"
    pure $ Album { id, title, artist, artistId, sortArtist, date }

getAlbums :: Aff (Array Album)
getAlbums = do
  result <- Http.get Http.ResponseFormat.json "/albums"
  case result of
    Left err -> fatal $ "Failed to retrieve albums: " <> Http.printError err
    Right response -> case Json.decodeJson response.body of
      Left err -> fatal $ "Failed to parse albums: " <> err
      Right albums -> pure $ sortWith (\(Album a) -> a.date) albums

newtype Artist = Artist
  { id :: ArtistId
  , name :: String
  , albums :: Array Album
  }

instance decodeJsonArtist :: DecodeJson Artist where
  decodeJson json = do
    obj        <- Json.decodeJson json
    id         <- map ArtistId $ Json.getField obj "id"
    name       <- Json.getField obj "name"
    albums     <- Json.getField obj "albums"
    pure $ Artist { id, name, albums }

getArtist :: ArtistId -> Aff Artist
getArtist (ArtistId artistId) = do
  result <- Http.get Http.ResponseFormat.json $ "/artists/" <> artistId
  case result of
    Left err -> fatal $ "Failed to retrieve artist: " <> Http.printError err
    Right response -> case Json.decodeJson response.body of
      Left err -> fatal $ "Failed to parse artist: " <> err
      Right artist -> pure artist

enqueueTrack :: TrackId -> Aff QueueId
enqueueTrack (TrackId trackId) = do
  result <- Http.put Http.ResponseFormat.json ("/queue/" <> trackId) Nothing
  case result of
    Left err -> fatal $ "Enqueue failed: " <> Http.printError err
    Right response -> case Json.decodeJson response.body of
      Left err -> fatal $ "Failed to enqueue track: " <> err
      Right queueId -> do
        Console.log $ "Enqueued track " <> trackId <> ", got queue id " <> queueId
        pure $ QueueId queueId

newtype Decibel = Decibel Number

derive instance decibelEq :: Eq Decibel
derive instance decibelOrd :: Ord Decibel

data VolumeChange = VolumeUp | VolumeDown

newtype Volume = Volume
  { volume :: Decibel
  }

instance decodeJsonVolume :: DecodeJson Volume where
  decodeJson json = do
    obj        <- Json.decodeJson json
    volDb      <- Json.getField obj "volume_db"
    pure $ Volume { volume: Decibel volDb }

getVolume :: Aff Volume
getVolume = do
  result <- Http.get Http.ResponseFormat.json "/volume"
  case result of
    Left err -> fatal $ "Failed to get volume: " <> Http.printError err
    Right response -> case Json.decodeJson response.body of
      Left err -> fatal $ "Failed to get volume: " <> err
      Right volume -> pure volume

changeVolume :: VolumeChange -> Aff Volume
changeVolume change =
  let
    dir = case change of
      VolumeUp -> "up"
      VolumeDown -> "down"
  in do
    result <- Http.post Http.ResponseFormat.json ("/volume/" <> dir) Nothing
    case result of
      Left err -> fatal $ "Failed to change volume: " <> Http.printError err
      Right response -> case Json.decodeJson response.body of
        Left err -> fatal $ "Failed to change volume: " <> err
        Right newVolume -> pure newVolume

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
  result <- Http.get Http.ResponseFormat.json ("/search?q=" <> query)
  case result of
    Left err -> fatal $ "Search failed: " <> Http.printError err
    Right response -> case Json.decodeJson response.body of
      Left err -> fatal $ "Failed to parse search results: " <> err
      Right results -> pure results

newtype QueuedTrackRaw = QueuedTrackRaw
  { queueId :: QueueId
  , trackId :: TrackId
  , title :: String
  , artist :: String
  , album :: String
  , albumId :: AlbumId
  , durationSeconds :: Int
  , positionSeconds :: Number
  , bufferedSeconds :: Number
  }

newtype QueuedTrack = QueuedTrack
  { queueId :: QueueId
  , trackId :: TrackId
  , title :: String
  , artist :: String
  , album :: String
  , albumId :: AlbumId
  , durationSeconds :: Int
  , startedAt :: Instant
  , refreshAt :: Instant
  }

instance decodeJsonQueuedTrackRaw :: DecodeJson QueuedTrackRaw where
  decodeJson json = do
    obj        <- Json.decodeJson json
    queueId    <- map QueueId $ Json.getField obj "queue_id"
    trackId    <- map TrackId $ Json.getField obj "track_id"
    title      <- Json.getField obj "title"
    artist     <- Json.getField obj "artist"
    album      <- Json.getField obj "album"
    albumId    <- map AlbumId $ Json.getField obj "album_id"
    durationSeconds <- Json.getField obj "duration_seconds"
    positionSeconds <- Json.getField obj "position_seconds"
    bufferedSeconds <- Json.getField obj "buffered_seconds"
    pure $ QueuedTrackRaw
      { queueId
      , trackId
      , title
      , artist
      , album
      , albumId
      , durationSeconds
      , positionSeconds
      , bufferedSeconds
      }

getQueue :: Aff (Array QueuedTrack)
getQueue = do
  t0 <- liftEffect $ Time.getCurrentInstant
  result <- Http.get Http.ResponseFormat.json "/queue"
  t1 <- liftEffect $ Time.getCurrentInstant

  let
    -- We assume that the request time is symmetric, so the time at which the
    -- server generated the response was the middle of t0 and t1. Treat all
    -- other offsets relative to that point in time.
    now = Time.mean t0 t1
    makeTimeAbsolute (QueuedTrackRaw track) = QueuedTrack
      { queueId: track.queueId
      , trackId: track.trackId
      , title: track.title
      , artist: track.artist
      , album: track.album
      , albumId: track.albumId
      , durationSeconds: track.durationSeconds
      , startedAt: Time.add (Time.fromSeconds $ -track.positionSeconds) now
        -- Add a little delay after we expect the buffer to run out (which
        -- likely means the track will stop), before we really refresh the
        -- queue. If there is some small offset in the time, we'd rather fetch
        -- a bit after the track stops, than being a bit too early and having to
        -- check a second time right away.
      , refreshAt: Time.add (Time.fromSeconds $ 0.1 + track.bufferedSeconds) now
      }

  case result of
    Left err -> fatal $ "Failed to retrieve queue: " <> Http.printError err
    Right response -> case Json.decodeJson response.body of
      Left err -> fatal $ "Failed to parse queue: " <> err
      Right results -> pure $ map makeTimeAbsolute results

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
  result <- Http.get Http.ResponseFormat.json $ "/album/" <> aid
  case result of
    Left err -> fatal $ "Failed to retrieve tracks: " <> Http.printError err
    Right response -> case decodeAlbumTracks response.body of
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

-- Load a path, return the body as string.
getString :: String -> Aff String
getString path = do
  result <- Http.get Http.ResponseFormat.string path
  case result of
    Left err -> fatal $ "Failed to retrieve " <> path <> ": " <> Http.printError err
    Right response -> case response.status of
      StatusCode 200 -> pure response.body
      _ -> fatal $ "Failed to retrieve " <> path <> ": " <> response.body
