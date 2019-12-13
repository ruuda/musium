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

import Dom as Dom
import Html (Html)
import Html as Html
import Model (Album, SearchAlbum (..), SearchTrack (..))
import Model as Model

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

renderSearchAlbum :: SearchAlbum -> Html Unit
renderSearchAlbum (SearchAlbum album) = do
  Html.li $ do
    Html.addClass "album"
    Html.img (Model.thumbUrl album.id) (album.title <> " by " <> album.artist) $ do
      Html.addClass "thumb"
    Html.span $ do
      Html.addClass "title"
      Html.text album.title
    Html.span $ do
      Html.addClass "artist"
      Html.text album.artist

renderSearchTrack :: SearchTrack -> Html Unit
renderSearchTrack (SearchTrack track) = do
  Html.li $ do
    Html.addClass "track"
    -- TODO: Turn album rendering into re-usable function.
    Html.img (Model.thumbUrl track.albumId) track.album $ do
      Html.addClass "thumb"
    Html.span $ do
      Html.addClass "title"
      Html.text track.title
    Html.span $ do
      Html.addClass "artist"
      Html.text track.artist

renderAlbumList :: Array Album -> Html Unit
renderAlbumList albums = do
  { searchBox, searchResultsList } <- Html.div $ do
    Html.setId "search"
    searchBox <- Html.div $ do
      Html.setId "search-query"
      Html.input "search" $ do
        Html.setId "search-box"
        ask

    searchResultsList <- Html.ul $ do
      Html.setId "search-results"
      ask

    pure { searchBox, searchResultsList }

  albumList <- Html.ul $ do
    Html.setId "album-list"
    buildTree 15 AlbumComponent.renderAlbum albums
    ask

  local (const searchBox) $ do
    Html.onInput $ \query -> do
      -- Fire off the search query and render it when it comes in.
      launchAff_ $ do
        Model.SearchResults result <- Model.search query
        Console.log $ "Received albums: " <> (show $ Array.length $ result.albums)
        Console.log $ "Received tracks: " <> (show $ Array.length $ result.tracks)
        liftEffect $ do
          Html.withElement searchResultsList $ do
            Html.clear
            traverse_ renderSearchAlbum result.albums
            traverse_ renderSearchTrack result.tracks

      -- Collapse the album list while searching.
      Html.withElement Dom.body $
        case query of
        "" -> do Html.removeClass "searching"
        _  -> do Html.addClass "searching"
