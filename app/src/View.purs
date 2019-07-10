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
import Prelude

import Model (Album)
import Html (Html)
import Html as Html

import AlbumComponent as AlbumComponent

-- Like `traverse_`, but if the input array is larger than the given chunk size,
-- split it up, with an additional <div>.
buildTree :: forall a. Int -> (a -> Html Unit) -> Array a -> Html Unit
buildTree n build xs =
  if Array.length xs <= n
    then traverse_ build xs
    else
      buildTree n Html.div
      $ map (\i -> traverse_ build $ Array.slice (i * n) ((i + 1) * n) xs)
      $ Array.range 0 (Array.length xs / n)

renderAlbumList :: Array Album -> Html Unit
renderAlbumList albums =
  Html.div $
    Html.ul $ do
      Html.setId "album-list"
      buildTree 15 AlbumComponent.renderAlbum albums
