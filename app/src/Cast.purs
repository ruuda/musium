-- Mindec -- Music metadata indexer
-- Copyright 2019 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module Cast
  ( CastSession
  , MediaSession
  , MusicTrackMetadata
  , QueueItem
  , getCastSession
  , getMediaSession
  , makeQueueItem
  , playTrack
  , queueTrack
  ) where

import Data.Function.Uncurried (Fn3, runFn3)
import Data.Maybe (Maybe (Just, Nothing))
import Effect (Effect)
import Effect.Aff (Aff)
import Effect.Aff.Compat (EffectFnAff, fromEffectFnAff)
import Prelude

type MusicTrackMetadata =
  { discNumber  :: Int
  , trackNumber :: Int
  , title       :: String
  , artist      :: String
  , albumTitle  :: String
  , albumArtist :: String
  , releaseDate :: String
  , imageUrl    :: String
  , trackUrl    :: String
  }

foreign import data QueueItem :: Type
foreign import data CastSession :: Type
foreign import data MediaSession :: Type

foreign import makeQueueItem :: MusicTrackMetadata -> QueueItem
foreign import getQueueItems :: MediaSession -> Effect (Array QueueItem)

foreign import getCastSessionImpl :: EffectFnAff CastSession
foreign import getMediaSessionImpl :: Fn3 (MediaSession -> Maybe MediaSession) (Maybe MediaSession) CastSession (Effect (Maybe MediaSession))
foreign import playTrackImpl :: Fn3 Unit CastSession QueueItem (EffectFnAff Unit)
foreign import queueTrackImpl :: Fn3 Unit MediaSession QueueItem (EffectFnAff Unit)

getCastSession :: Aff CastSession
getCastSession = fromEffectFnAff getCastSessionImpl

getMediaSession :: CastSession -> Effect (Maybe MediaSession)
getMediaSession castSession = runFn3 getMediaSessionImpl Just Nothing castSession

playTrack :: CastSession -> QueueItem -> Aff Unit
playTrack castSession queueItem = fromEffectFnAff $ runFn3 playTrackImpl unit castSession queueItem

queueTrack :: MediaSession -> QueueItem -> Aff Unit
queueTrack mediaSession queueItem = fromEffectFnAff $ runFn3 queueTrackImpl unit mediaSession queueItem
