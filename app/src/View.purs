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
import Data.Foldable (for_, traverse_)
import Data.String.CodeUnits as CodeUnits
import Effect.Aff (launchAff_)
import Effect.Class (liftEffect)
import Effect.Class.Console as Console
import Prelude

import Dom as Dom
import Html (Html)
import Html as Html
import Model (Album, SearchArtist (..), SearchAlbum (..), SearchTrack (..))
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

renderSearchArtist :: SearchArtist -> Html Unit
renderSearchArtist (SearchArtist artist) = do
  Html.li $ do
    Html.addClass "artist"
    Html.div $ do
      Html.addClass "name"
      Html.text artist.name
    Html.div $ do
      Html.addClass "discography"
      for_ artist.albums $ \albumId -> do
        Html.img (Model.thumbUrl albumId) ("An album by " <> artist.name) $ pure unit

-- TODO: Deduplicate between here and album component.
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
      Html.text $ album.artist <> " "
      Html.span $ do
        Html.addClass "date"
        Html.setTitle album.date
        -- The date is of the form YYYY-MM-DD in ascii, so we can safely take
        -- the first 4 characters to get the year.
        Html.text (CodeUnits.take 4 album.date)

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
  { searchBox, searchResultsBox } <- Html.div $ do
    Html.setId "search"
    searchBox <- Html.div $ do
      Html.setId "search-query"
      Html.input "search" $ do
        Html.setId "search-box"
        ask

    searchResultsBox <- Html.div $ do
      Html.setId "search-results"
      ask

    pure { searchBox, searchResultsBox }

  albumList <- Html.ul $ do
    Html.setId "album-list"
    buildTree 15 AlbumComponent.renderAlbum albums
    ask

  local (const searchBox) $ do
    Html.onInput $ \query -> do
      -- Fire off the search query and render it when it comes in.
      launchAff_ $ do
        Model.SearchResults result <- Model.search query
        Console.log $ "Received artists: " <> (show $ Array.length $ result.artists)
        Console.log $ "Received albums:  " <> (show $ Array.length $ result.albums)
        Console.log $ "Received tracks:  " <> (show $ Array.length $ result.tracks)
        liftEffect $ do
          Html.withElement searchResultsBox $ do
            Html.clear

            when (not $ Array.null result.artists) $ do
              Html.h2 $ Html.text "Artists"
              Html.div $ do
                Html.setId "search-artists"
                Html.ul $ for_ result.artists renderSearchArtist

            when (not $ Array.null result.albums) $ do
              Html.h2 $ Html.text "Albums"
              Html.div $ do
                Html.setId "search-albums"
                Html.ul $ for_ result.albums renderSearchAlbum

            when (not $ Array.null result.tracks) $ do
              Html.h2 $ Html.text "Tracks"
              Html.div $ do
                Html.setId "search-tracks"
                Html.ul $ for_ result.tracks renderSearchTrack

      -- Collapse the album list while searching.
      Html.withElement Dom.body $
        case query of
        "" -> do Html.removeClass "searching"
        _  -> do Html.addClass "searching"
