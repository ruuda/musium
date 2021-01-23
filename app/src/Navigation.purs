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

import Model (ArtistId (..), AlbumId (..))

data Location
  = Library
  | Artist ArtistId
  | Album AlbumId
  | NowPlaying
  | Search

derive instance eqLocation :: Eq Location

toUrl :: Location -> String
toUrl loc = case loc of
  Library -> "/"
  Artist (ArtistId id) -> "/?artist=" <> id
  Album (AlbumId id) -> "/?album=" <> id
  NowPlaying -> "/?current"
  Search -> "/?search"

fromUrl :: String -> Location
fromUrl url =
  case stripPrefix (Pattern "/?artist=") url of
    Just artistId -> Artist (ArtistId artistId)
    Nothing -> case stripPrefix (Pattern "/?album=") url of
      Just albumId -> Album (AlbumId albumId)
      Nothing       -> case url of
        "/?current" -> NowPlaying
        "/?search"  -> Search
        "/"         -> Library
        _           -> Library
