-- Mindec -- Music metadata indexer
-- Copyright 2019 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module View
  ( renderAlbumList
  ) where

import Control.Monad.Reader.Class (ask, local)
import Data.Array as Array
import Data.Foldable (traverse_)
import Effect.Aff (launchAff_)
import Effect.Class (liftEffect)
import Effect.Class.Console as Console
import Prelude

import Model (Album)
import Model as Model
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
renderAlbumList albums = do
  Html.div $ do
    Html.setId "search"
    searchBox <- Html.div $ do
      Html.setId "search-query"
      Html.input "search" $ do
        Html.setId "search-box"
        ask

    resultsBox <- Html.div $ do
      Html.setId "search-results"
      Html.ul $ ask

    local (const searchBox) $ do
      Html.onInput $ \query -> do
        Console.log $ "Search: " <> query
        launchAff_ $ do
          Model.SearchResults result <- Model.search query
          Console.log $ "Received albums: " <> (show $ Array.length $ result.albums)
          Console.log $ "Received tracks: " <> (show $ Array.length $ result.tracks)
          liftEffect $ do
            Html.appendTo resultsBox $ do
              traverse_ (\(Model.SearchAlbum album) -> Html.ul $ Html.text album.title) result.albums
              traverse_ (\(Model.SearchTrack track) -> Html.ul $ Html.text track.title) result.tracks

  Html.div $
    Html.ul $ do
      Html.setId "album-list"
      buildTree 15 AlbumComponent.renderAlbum albums
