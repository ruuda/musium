-- Musium -- Music playback daemon with web-based library browser
-- Copyright 2020 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module Search
  ( SearchElements
  , new
  , focus
  , clear
  ) where

import Control.Monad.Reader.Class (ask, local)
import Data.Array as Array
import Data.Foldable (for_)
import Data.String.CodeUnits as CodeUnits
import Effect (Effect)
import Effect.Aff (Aff, launchAff_)
import Effect.Class (liftEffect)
import Effect.Class.Console as Console
import Prelude

import Dom (Element)
import Dom as Dom
import Event (Event, HistoryMode (RecordHistory))
import Event as Event
import Html (Html)
import Html as Html
import Model (SearchArtist (..), SearchAlbum (..), SearchTrack (..))
import Model as Model
import Navigation as Navigation

type SearchElements =
  { searchBox :: Element
  , resultBox :: Element
  }

renderSearchArtist :: (Event -> Aff Unit) -> SearchArtist -> Html Unit
renderSearchArtist postEvent (SearchArtist artist) = do
  Html.li $ do
    Html.addClass "artist"
    Html.div $ do
      Html.addClass "name"
      Html.text artist.name
    Html.div $ do
      Html.addClass "discography"
      for_ artist.albums $ \albumId -> do
        Html.img (Model.thumbUrl albumId) ("An album by " <> artist.name) $ pure unit

    Html.onClick $ launchAff_ $ postEvent $ Event.NavigateTo
      (Navigation.Artist $ artist.id)
      RecordHistory

-- TODO: Deduplicate between here and album component.
renderSearchAlbum :: (Event -> Aff Unit) -> SearchAlbum -> Html Unit
renderSearchAlbum postEvent (SearchAlbum album) = do
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
        Html.setTitle album.releaseDate
        -- The date is of the form YYYY-MM-DD in ascii, so we can safely take
        -- the first 4 characters to get the year.
        Html.text (CodeUnits.take 4 album.releaseDate)

    Html.onClick $ launchAff_ $ postEvent $ Event.NavigateTo
      (Navigation.Album $ album.id)
      RecordHistory

renderSearchTrack :: (Event -> Aff Unit) -> SearchTrack -> Html Unit
renderSearchTrack postEvent (SearchTrack track) = do
  Html.li $ do
    Html.addClass "track"
    Html.img (Model.thumbUrl track.albumId) track.album $ do
      Html.addClass "thumb"
    Html.span $ do
      Html.addClass "title"
      Html.text track.title
    Html.span $ do
      Html.addClass "artist"
      Html.text track.artist

    -- TODO: Add a way to emphasize that track after navigating to the album.
    Html.onClick $ launchAff_ $ postEvent $ Event.NavigateTo
      (Navigation.Album $ track.albumId)
      RecordHistory

new :: (Event -> Aff Unit) -> Html SearchElements
new postEvent = do
  searchBox <- Html.input "search" $ do
    Html.setId "search-box"
    Html.setType "search"
    ask

  resultBox <- Html.div $ do
    Html.setId "search-results"
    ask

  local (const searchBox) $ do
    Html.onInput $ \query -> do
      -- Fire off the search query and render it when it comes in.
      -- TODO: Pass these through the event loop, to ensure that the result
      -- matches the query, and perhaps for caching as well.
      launchAff_ $ do
        Model.SearchResults result <- Model.search query
        Console.log $ "Received artists: " <> (show $ Array.length $ result.artists)
        Console.log $ "Received albums:  " <> (show $ Array.length $ result.albums)
        Console.log $ "Received tracks:  " <> (show $ Array.length $ result.tracks)
        liftEffect $ do
          Html.withElement resultBox $ do
            Html.clear
            Html.div $ do
              Html.addClass "search-results-list"

              when (not $ Array.null result.artists) $ do
                Html.div $ do
                  Html.setId "search-artists"
                  Html.h2 $ Html.text "Artists"
                  -- Limit the number of results rendered at once to keep search
                  -- responsive. TODO: Render overflow button.
                  Html.ul $ for_ (Array.take 10 result.artists) $ renderSearchArtist postEvent

              when (not $ Array.null result.albums) $ do
                Html.div $ do
                  Html.setId "search-albums"
                  Html.h2 $ Html.text "Albums"
                  -- Limit the number of results rendered at once to keep search
                  -- responsive. TODO: Render overflow button.
                  Html.ul $ for_ (Array.take 25 result.albums) $ renderSearchAlbum postEvent

              when (not $ Array.null result.tracks) $ do
                Html.div $ do
                  Html.setId "search-tracks"
                  Html.h2 $ Html.text "Tracks"
                  -- Limit the number of results rendered at once to keep search
                  -- responsive. TODO: Render overflow button.
                  Html.ul $ for_ (Array.take 25 result.tracks) $ renderSearchTrack postEvent

  pure $ { searchBox, resultBox }

clear :: SearchElements -> Effect Unit
clear elements = do
  Dom.setValue "" elements.searchBox
  Dom.clearElement elements.resultBox

focus :: SearchElements -> Effect Unit
focus elements = Dom.focusElement elements.searchBox
