-- Mindec -- Music metadata indexer
-- Copyright 2019 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module View
  ( renderAlbumList
  ) where

import Data.Array as Array
import Data.Foldable (traverse_)
import Data.Maybe (Maybe (..))
import Data.String as String
import Data.Symbol (SProxy (..))
import Effect.Aff.Class (class MonadAff)
import Effect.Class.Console as Console
import Prelude

import Model (Album (..), AlbumId)
import Model as Model
import Html (Html)
import Html as Html

import AlbumComponent as AlbumComponent

renderAlbumList :: Array Album -> Html Unit
renderAlbumList albums =
  Html.div $
    Html.ul $ do
      Html.setId "album-list"
      traverse_ AlbumComponent.renderAlbum' albums
