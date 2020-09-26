-- Musium -- Music playback daemon with web-based library browser
-- Copyright 2020 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module Navigation
  ( Location (..)
  , toUrl
  , fromUrl
  ) where

import Data.Maybe (Maybe (Just, Nothing))
import Data.String (Pattern (..), stripPrefix)
import Prelude

import Model (AlbumId (..))

data Location
  = Library
  | NowPlaying
  | Album AlbumId

derive instance eqLocation :: Eq Location

toUrl :: Location -> String
toUrl loc = case loc of
  Library -> "/"
  NowPlaying -> "/?now"
  Album (AlbumId id) -> "/?album=" <> id

fromUrl :: String -> Location
fromUrl url = case stripPrefix (Pattern "/?album=") url of
  Just albumId -> Album (AlbumId albumId)
  Nothing      -> case url of
    "/?now"    -> NowPlaying
    "/"        -> Library
    _          -> Library
