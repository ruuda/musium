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
  , Hertz (..)
  , QueueId (..)
  , QueuedTrack (..)
  , Rating (..)
  , ScanStage (..)
  , ScanStatus (..)
  , SearchAlbum (..)
  , SearchArtist (..)
  , SearchResults (..)
  , SearchTrack (..)
  , Stats (..)
  , Track (..)
  , TrackId (..)
  , PlayerParams (..)
  , VolumeChange (..)
  , coverUrl
  , changeCutoff
  , changeVolume
  , clearQueue
  , enqueueTrack
  , formatDurationSeconds
  , getAlbums
  , getArtist
  , getPlayerParams
  , getQueue
  , getScanStatus
  , getStats
  , getString
  , getTracks
  , originalReleaseYear
  , search
  , setRating
  , shuffleQueue
  , startScan
  , thumbUrl
  , timeLeft
  , trackUrl
  , waveformUrl
  ) where

import Prelude

import Affjax.Web as Http
import Affjax.ResponseFormat as Http.ResponseFormat
import Affjax.StatusCode (StatusCode (..))
import Control.Monad.Error.Class (class MonadThrow, throwError)
import Data.Argonaut.Core (Json)
import Data.Argonaut.Decode (decodeJson, getField) as Json
import Data.Argonaut.Decode.Class (class DecodeJson)
import Data.Argonaut.Decode.Error (JsonDecodeError (AtKey, UnexpectedValue, MissingValue), printJsonDecodeError)
import Data.Array as Array
import Data.Array.NonEmpty (NonEmptyArray)
import Data.Array.NonEmpty as NonEmptyArray
import Data.Either (Either (..))
import Data.Int (rem)
import Data.Int as Int
import Data.Maybe (Maybe (Just, Nothing))
import Data.String as String
import Effect.Aff (Aff)
import Effect.Class (liftEffect)
import Effect.Class.Console as Console
import Effect.Exception (Error, error)
import Time as Time
import Time (Duration, Instant)

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

newtype Rating = Rating Int

derive instance ratingEq :: Eq Rating
derive instance ratingOrd :: Ord Rating

instance showRating :: Show Rating where
  show (Rating n) = show n

thumbUrl :: AlbumId -> String
thumbUrl (AlbumId id) = "/api/thumb/" <> id

coverUrl :: AlbumId -> String
coverUrl (AlbumId id) = "/api/cover/" <> id

waveformUrl :: TrackId -> String
waveformUrl (TrackId id) = "/api/waveform/" <> id

trackUrl :: TrackId -> String
trackUrl (TrackId id) = "/api/track/" <> id <> ".flac"

newtype Album = Album
  { id :: AlbumId
  , title :: String
  , artist :: String
  , artistIds :: NonEmptyArray ArtistId
  , releaseDate :: String
  , firstSeen :: String
  , color :: String
  , discoverScore :: Number
  , trendingScore :: Number
  , forNowScore :: Number
  }

instance decodeJsonAlbum :: DecodeJson Album where
  decodeJson json = do
    obj        <- Json.decodeJson json
    id         <- map AlbumId $ Json.getField obj "id"
    title      <- Json.getField obj "title"
    artistIdsM <- map (map ArtistId) $ Json.getField obj "artist_ids"
    artistIds  <- case NonEmptyArray.fromArray artistIdsM of
      Just xs -> pure xs
      Nothing -> Left $ AtKey "artist_ids" MissingValue
    artist        <- Json.getField obj "artist"
    releaseDate   <- Json.getField obj "release_date"
    firstSeen     <- Json.getField obj "first_seen"
    color         <- Json.getField obj "color"
    discoverScore <- Json.getField obj "discover_score"
    trendingScore <- Json.getField obj "trending_score"
    forNowScore <- Json.getField obj "for_now_score"
    pure $ Album
      { id
      , title
      , artist
      , artistIds
      , releaseDate
      , firstSeen
      , color
      , discoverScore
      , trendingScore
      , forNowScore
      }

getAlbums :: Aff (Array Album)
getAlbums = do
  result <- Http.get Http.ResponseFormat.json "/api/albums"
  case result of
    Left err -> fatal $ "Failed to retrieve albums: " <> Http.printError err
    Right response -> case Json.decodeJson response.body of
      Left err -> fatal $ "Failed to parse albums: " <> printJsonDecodeError err
      Right albums -> pure albums

newtype ArtistJson = ArtistJson
  { name :: String
  , albums :: Array Album
  }

type Artist =
  { id :: ArtistId
  , name :: String
  , albums :: Array Album
  }

instance decodeJsonArtist :: DecodeJson ArtistJson where
  decodeJson json = do
    obj        <- Json.decodeJson json
    name       <- Json.getField obj "name"
    albums     <- Json.getField obj "albums"
    pure $ ArtistJson { name, albums }

getArtist :: ArtistId -> Aff Artist
getArtist (ArtistId artistId) = do
  result <- Http.get Http.ResponseFormat.json $ "/api/artist/" <> artistId
  case result of
    Left err -> fatal $ "Failed to retrieve artist: " <> Http.printError err
    Right response -> case Json.decodeJson response.body of
      Left err -> fatal $ "Failed to parse artist: " <> printJsonDecodeError err
      Right (ArtistJson artist) -> pure $
        { id: ArtistId artistId
        , name: artist.name
        , albums: Array.reverse artist.albums
        }

enqueueTrack :: TrackId -> Aff QueueId
enqueueTrack (TrackId trackId) = do
  result <- Http.put Http.ResponseFormat.json ("/api/queue/" <> trackId) Nothing
  case result of
    Left err -> fatal $ "Enqueue failed: " <> Http.printError err
    Right response -> case Json.decodeJson response.body of
      Left err -> fatal $ "Failed to enqueue track: " <> printJsonDecodeError err
      Right queueId -> do
        Console.log $ "Enqueued track " <> trackId <> ", got queue id " <> queueId
        pure $ QueueId queueId

newtype Decibel = Decibel Number
derive instance decibelEq :: Eq Decibel
derive instance decibelOrd :: Ord Decibel

newtype Hertz = Hertz Number
derive instance hertzEq :: Eq Hertz
derive instance hertzOrd :: Ord Hertz

data VolumeChange = VolumeUp | VolumeDown

newtype PlayerParams = PlayerParams
  { volume :: Decibel
  , highPassCutoff :: Hertz
  }

instance decodeJsonParams :: DecodeJson PlayerParams where
  decodeJson json = do
    obj        <- Json.decodeJson json
    volume      <- Json.getField obj "volume_db"
    cutoff      <- Json.getField obj "high_pass_cutoff_hz"
    pure $ PlayerParams
      { volume: Decibel volume
      , highPassCutoff: Hertz cutoff
      }

getPlayerParams :: Aff PlayerParams
getPlayerParams = do
  result <- Http.get Http.ResponseFormat.json "/api/volume"
  case result of
    Left err -> fatal $ "Failed to get volume: " <> Http.printError err
    Right response -> case Json.decodeJson response.body of
      Left err -> fatal $ "Failed to get volume: " <> printJsonDecodeError err
      Right ps -> pure ps

changeVolume :: VolumeChange -> Aff PlayerParams
changeVolume change =
  let
    dir = case change of
      VolumeUp -> "up"
      VolumeDown -> "down"
  in do
    result <- Http.post Http.ResponseFormat.json ("/api/volume/" <> dir) Nothing
    case result of
      Left err -> fatal $ "Failed to change volume: " <> Http.printError err
      Right response -> case Json.decodeJson response.body of
        Left err -> fatal $ "Failed to change volume: " <> printJsonDecodeError err
        Right newParams -> pure newParams

-- Change the cutoff of the high-pass filter. We ~~abuse~~ resue the `VolumeChange` type.
changeCutoff :: VolumeChange -> Aff PlayerParams
changeCutoff change =
  let
    dir = case change of
      VolumeUp -> "up"
      VolumeDown -> "down"
  in do
    result <- Http.post Http.ResponseFormat.json ("/api/filter/" <> dir) Nothing
    case result of
      Left err -> fatal $ "Failed to change filter cutoff: " <> Http.printError err
      Right response -> case Json.decodeJson response.body of
        Left err -> fatal $ "Failed to change filter cutoff: " <> printJsonDecodeError err
        Right newParams -> pure newParams

data ScanStage
  = ScanDiscovering
  | ScanPreProcessingMetadata
  | ScanExtractingMetadata
  | ScanIndexingMetadata
  | ScanPreProcessingLoudness
  | ScanAnalyzingLoudness
  | ScanPreProcessingThumbnails
  | ScanGeneratingThumbnails
  | ScanLoadingThumbnails
  | ScanReloading
  | ScanDone

derive instance eqScanStage :: Eq ScanStage
derive instance ordScanStage :: Ord ScanStage

instance decodeJsonScanStage :: DecodeJson ScanStage where
  decodeJson json = do
    str <- Json.decodeJson json
    case str of
      "discovering"              -> pure ScanDiscovering
      "preprocessing_metadata"   -> pure ScanPreProcessingMetadata
      "extracting_metadata"      -> pure ScanExtractingMetadata
      "indexing_metadata"        -> pure ScanIndexingMetadata
      "preprocessing_loudness"   -> pure ScanPreProcessingLoudness
      "analyzing_loudness"       -> pure ScanAnalyzingLoudness
      "preprocessing_thumbnails" -> pure ScanPreProcessingThumbnails
      "generating_thumbnails"    -> pure ScanGeneratingThumbnails
      "loading_thumbnails"       -> pure ScanLoadingThumbnails
      "reloading"                -> pure ScanReloading
      "done"                     -> pure ScanDone
      _ -> Left $ UnexpectedValue json

newtype ScanStatus = ScanStatus
  { stage :: ScanStage
  , filesDiscovered :: Int
  , filesToProcessMetadata :: Int
  , filesProcessedMetadata :: Int
  , tracksToProcessLoudness :: Int
  , tracksProcessedLoudness :: Int
  , albumsToProcessLoudness :: Int
  , albumsProcessedLoudness :: Int
  , filesToProcessThumbnails :: Int
  , filesProcessedThumbnails :: Int
  }

instance decodeJsonScanStatus :: DecodeJson ScanStatus where
  decodeJson json = do
    obj                      <- Json.decodeJson json
    stage                    <- Json.getField obj "stage"
    filesDiscovered          <- Json.getField obj "files_discovered"
    filesToProcessMetadata   <- Json.getField obj "files_to_process_metadata"
    filesProcessedMetadata   <- Json.getField obj "files_processed_metadata"
    tracksToProcessLoudness  <- Json.getField obj "tracks_to_process_loudness"
    tracksProcessedLoudness  <- Json.getField obj "tracks_processed_loudness"
    albumsToProcessLoudness  <- Json.getField obj "albums_to_process_loudness"
    albumsProcessedLoudness  <- Json.getField obj "albums_processed_loudness"
    filesToProcessThumbnails <- Json.getField obj "files_to_process_thumbnails"
    filesProcessedThumbnails <- Json.getField obj "files_processed_thumbnails"
    pure $ ScanStatus
      { stage
      , filesDiscovered
      , filesToProcessMetadata
      , filesProcessedMetadata
      , tracksToProcessLoudness
      , tracksProcessedLoudness
      , albumsToProcessLoudness
      , albumsProcessedLoudness
      , filesToProcessThumbnails
      , filesProcessedThumbnails
      }

getScanStatus :: Aff (Maybe ScanStatus)
getScanStatus = do
  result <- Http.get Http.ResponseFormat.json "/api/scan/status"
  case result of
    Left err -> fatal $ "Failed to get scan status: " <> Http.printError err
    Right response -> case Json.decodeJson response.body of
      Left err -> fatal $ "Failed to get scan status: " <> printJsonDecodeError err
      Right status -> pure status

startScan :: Aff ScanStatus
startScan = do
  result <- Http.post Http.ResponseFormat.json "/api/scan/start" Nothing
  case result of
    Left err -> fatal $ "Failed to get scan status: " <> Http.printError err
    Right response -> case Json.decodeJson response.body of
      Left err -> fatal $ "Failed to get scan status: " <> printJsonDecodeError err
      Right status -> pure status

newtype Stats = Stats
  { tracks :: Int
  , albums :: Int
  , artists :: Int
  }

instance decodeJsonStats :: DecodeJson Stats where
  decodeJson json = do
    obj     <- Json.decodeJson json
    tracks  <- Json.getField obj "tracks"
    albums  <- Json.getField obj "albums"
    artists <- Json.getField obj "artists"
    pure $ Stats { tracks, albums, artists }

getStats :: Aff Stats
getStats = do
  result <- Http.get Http.ResponseFormat.json "/api/stats"
  case result of
    Left err -> fatal $ "Failed to get stats: " <> Http.printError err
    Right response -> case Json.decodeJson response.body of
      Left err -> fatal $ "Failed to get stats: " <> printJsonDecodeError err
      Right stats -> pure stats

setRating :: TrackId -> Rating -> Aff Unit
setRating tid r = do
  result <- Http.put Http.ResponseFormat.json
    ("/api/track/" <> (show tid) <> "/rating/" <> (show r))
    Nothing
  case result of
    Left err -> fatal $ "Failed to set rating: " <> Http.printError err
    Right _ -> pure unit

newtype SearchArtist = SearchArtist
  { id :: ArtistId
  , name :: String
  , albums :: Array AlbumId
  }

newtype SearchAlbum = SearchAlbum
  { id :: AlbumId
  , title :: String
  , artist :: String
  , releaseDate :: String
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
    obj         <- Json.decodeJson json
    id          <- map AlbumId $ Json.getField obj "id"
    title       <- Json.getField obj "title"
    artist      <- Json.getField obj "artist"
    releaseDate <- Json.getField obj "release_date"
    pure $ SearchAlbum { id, title, artist, releaseDate }

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
  result <- Http.get Http.ResponseFormat.json ("/api/search?q=" <> query)
  case result of
    Left err -> fatal $ "Search failed: " <> Http.printError err
    Right response -> case Json.decodeJson response.body of
      Left err -> fatal $ "Failed to parse search results: " <> printJsonDecodeError err
      Right results -> pure results

newtype QueuedTrackRaw = QueuedTrackRaw
  { queueId :: QueueId
  , trackId :: TrackId
  , title :: String
  , artist :: String
  , album :: String
  , albumId :: AlbumId
  , albumArtistIds :: NonEmptyArray ArtistId
  , releaseDate :: String
  , rating :: Rating
  , durationSeconds :: Int
  , positionSeconds :: Number
  , bufferedSeconds :: Number
  , isBuffering :: Boolean
  }

newtype QueuedTrack = QueuedTrack
  { queueId :: QueueId
  , trackId :: TrackId
  , title :: String
  , artist :: String
  , album :: String
  , albumId :: AlbumId
  , albumArtistIds :: NonEmptyArray ArtistId
  , releaseDate :: String
  , rating :: Rating
  , durationSeconds :: Int
  , positionSeconds :: Number
  , bufferedSeconds :: Number
  , isBuffering :: Boolean
  , startedAt :: Instant
  , refreshAt :: Instant
  }

-- For a playing track, the time left until it stops playing.
timeLeft :: Instant -> QueuedTrack -> Duration
timeLeft now (QueuedTrack track) =
  let
    posSeconds = Time.toSeconds $ now `Time.subtract` track.startedAt
  in
    Time.fromSeconds $ (Int.toNumber track.durationSeconds) - posSeconds

instance decodeJsonQueuedTrackRaw :: DecodeJson QueuedTrackRaw where
  decodeJson json = do
    obj             <- Json.decodeJson json
    queueId         <- map QueueId $ Json.getField obj "queue_id"
    trackId         <- map TrackId $ Json.getField obj "track_id"
    title           <- Json.getField obj "title"
    artist          <- Json.getField obj "artist"
    album           <- Json.getField obj "album"
    albumId         <- map AlbumId $ Json.getField obj "album_id"
    albumArtistIdsM <- map (map ArtistId) $ Json.getField obj "album_artist_ids"
    albumArtistIds  <- case NonEmptyArray.fromArray albumArtistIdsM of
      Just xs -> pure xs
      Nothing -> Left $ AtKey "album_artist_ids" MissingValue
    releaseDate     <- Json.getField obj "release_date"
    rating          <- map Rating $ Json.getField obj "rating"
    durationSeconds <- Json.getField obj "duration_seconds"
    positionSeconds <- Json.getField obj "position_seconds"
    bufferedSeconds <- Json.getField obj "buffered_seconds"
    isBuffering     <- Json.getField obj "is_buffering"
    pure $ QueuedTrackRaw
      { queueId
      , trackId
      , title
      , artist
      , album
      , albumId
      , albumArtistIds
      , releaseDate
      , rating
      , durationSeconds
      , positionSeconds
      , bufferedSeconds
      , isBuffering
      }

getQueueGeneric
  :: Aff (Either Http.Error (Http.Response Json))
  -> Aff (Array QueuedTrack)
getQueueGeneric doRequest = do
  t0 <- liftEffect $ Time.getCurrentInstant
  result <- doRequest
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
      , albumArtistIds: track.albumArtistIds
      , releaseDate: track.releaseDate
      , rating: track.rating
      , durationSeconds: track.durationSeconds
      , positionSeconds: track.positionSeconds
      , bufferedSeconds: track.bufferedSeconds
      , isBuffering: track.isBuffering
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
      Left err -> fatal $ "Failed to parse queue: " <> printJsonDecodeError err
      Right results -> pure $ map makeTimeAbsolute results

getQueue :: Aff (Array QueuedTrack)
getQueue = getQueueGeneric $
  Http.get Http.ResponseFormat.json "/api/queue"

shuffleQueue :: Aff (Array QueuedTrack)
shuffleQueue = getQueueGeneric $
  Http.post Http.ResponseFormat.json "/api/queue/shuffle" Nothing

clearQueue :: Aff (Array QueuedTrack)
clearQueue = getQueueGeneric $
  Http.post Http.ResponseFormat.json "/api/queue/clear" Nothing

newtype Track = Track
  { id :: TrackId
  , discNumber :: Int
  , trackNumber :: Int
  , title :: String
  , artist :: String
  , durationSeconds :: Int
  , rating :: Rating
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
    rating          <- map Rating $ Json.getField obj "rating"
    pure $ Track
      { id
      , discNumber
      , trackNumber
      , title
      , artist
      , durationSeconds
      , rating
      }

decodeAlbumTracks :: Json -> Either JsonDecodeError (Array Track)
decodeAlbumTracks json = do
  obj <- Json.decodeJson json
  Json.getField obj "tracks"

getTracks :: AlbumId -> Aff (Array Track)
getTracks (AlbumId aid) = do
  result <- Http.get Http.ResponseFormat.json $ "/api/album/" <> aid
  case result of
    Left err -> fatal $ "Failed to retrieve tracks: " <> Http.printError err
    Right response -> case decodeAlbumTracks response.body of
      Left err -> fatal $ "Failed to parse tracks: " <> printJsonDecodeError err
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
originalReleaseYear (Album album) = String.take 4 album.releaseDate

-- Load a path, return the body as string.
getString :: String -> Aff String
getString path = do
  result <- Http.get Http.ResponseFormat.string path
  case result of
    Left err -> fatal $ "Failed to retrieve " <> path <> ": " <> Http.printError err
    Right response -> case response.status of
      StatusCode 200 -> pure response.body
      _ -> fatal $ "Failed to retrieve " <> path <> ": " <> response.body
