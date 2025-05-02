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
  , renderSearchResults
  ) where

import Control.Monad.Reader.Class (ask, local)
import Data.Array as Array
import Data.Foldable (for_)
import Data.Maybe (Maybe (..))
import Data.String.CodeUnits as CodeUnits
import Effect (Effect)
import Effect.Aff (Aff, launchAff_)
import Effect.Class (liftEffect)
import Foreign.Object (Object)
import Foreign.Object as Object
import Prelude

import Dom (Element)
import Dom as Dom
import Event (Event, HistoryMode (RecordHistory), SearchSeq (SearchSeq))
import Event as Event
import Html (Html)
import Html as Html
import Model (Album (..), AlbumId (..), SearchArtist (..), SearchAlbum (..), SearchResults (..), SearchTrack (..))
import Model as Model
import Navigation as Navigation
import Var as Var

type SearchElements =
  { searchBox :: Element
  , resultBox :: Element
  }

-- Look up the album by id in the album collection. If we found it, set the
-- background color to that album's color. Intended to be curried.
setAlbumColor :: Object Album -> AlbumId -> Html Unit
setAlbumColor albumsById (AlbumId id) = case Object.lookup id albumsById of
  Nothing -> pure unit
  Just (Album album) -> Html.setBackgroundColor album.color

renderSearchArtist
  :: (Event -> Aff Unit)
  -> (AlbumId -> Html Unit)
  -> SearchArtist
  -> Html Unit
renderSearchArtist postEvent setColor (SearchArtist artist) = do
  Html.li $ do
    Html.addClass "artist"
    Html.div $ do
      Html.addClass "name"
      Html.text artist.name
    Html.div $ do
      Html.addClass "discography"
      for_ artist.albums $ \albumId -> Html.img
        (Model.thumbUrl albumId)
        ("An album by " <> artist.name)
        (setColor albumId)

    Html.onClick $ launchAff_ $ postEvent $ Event.NavigateTo
      (Navigation.Artist $ artist.id)
      RecordHistory

-- TODO: Deduplicate between here and album component.
renderSearchAlbum
 :: (Event -> Aff Unit)
 -> (AlbumId -> Html Unit)
 -> SearchAlbum
 -> Html Unit
renderSearchAlbum postEvent setColor (SearchAlbum album) = do
  Html.li $ do
    Html.addClass "album"
    Html.img (Model.thumbUrl album.id) (album.title <> " by " <> album.artist) $ do
      setColor album.id
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

renderSearchTrack
  :: (Event -> Aff Unit)
  -> (AlbumId -> Html Unit)
  -> SearchTrack
  -> Html Unit
renderSearchTrack postEvent setColor (SearchTrack track) = do
  Html.li $ do
    Html.addClass "track"
    Html.img (Model.thumbUrl track.albumId) track.album $ do
      setColor track.albumId
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
    -- We maintain the search sequence number here, because the input handler
    -- runs as an Effect rather than Aff, so we can be sure that the sequence
    -- numbers match the order of the input events. In the main loop, we only
    -- process search results if they are for a newer search than the last one
    -- we processed, to ensure that a slow search query that arrives later does
    -- not overwrite current search results. (That can happen especially at the
    -- beginning, as a short query string matches more, so the response is
    -- larger and takes longer to serialize/transfer/deserialize.)
    searchSeq <- liftEffect $ Var.create 0
    Html.onInput $ \query -> do
      currentSeq <- Var.get searchSeq
      let nextSeq = currentSeq + 1
      Var.set searchSeq nextSeq
      launchAff_ $ postEvent $ Event.Search (SearchSeq nextSeq) query

  pure $ { searchBox, resultBox }

renderSearchResults
  :: (Event -> Aff Unit)
  -> SearchElements
  -> Object Album
  -> SearchResults
  -> Effect Unit
renderSearchResults postEvent elements albumsById (SearchResults result) =
  let
    setColor = setAlbumColor albumsById
  in
    Html.withElement elements.resultBox $ do
      Html.clear
      Html.div $ do
        Html.addClass "search-results-list"

        when (not $ Array.null result.artists) $ do
          Html.div $ do
            Html.setId "search-artists"
            Html.h2 $ Html.text "Artists"
            -- Limit the number of results rendered at once to keep search
            -- responsive. TODO: Render overflow button.
            Html.ul $ for_ (Array.take 10 result.artists) $
              renderSearchArtist postEvent setColor

        when (not $ Array.null result.albums) $ do
          Html.div $ do
            Html.setId "search-albums"
            Html.h2 $ Html.text "Albums"
            -- Limit the number of results rendered at once to keep search
            -- responsive. TODO: Render overflow button.
            Html.ul $ for_ (Array.take 25 result.albums) $
              renderSearchAlbum postEvent setColor

        when (not $ Array.null result.tracks) $ do
          Html.div $ do
            Html.setId "search-tracks"
            Html.h2 $ Html.text "Tracks"
            -- Limit the number of results rendered at once to keep search
            -- responsive. TODO: Render overflow button.
            Html.ul $ for_ (Array.take 25 result.tracks) $
              renderSearchTrack postEvent setColor

clear :: SearchElements -> Effect Unit
clear elements = do
  Dom.setValue "" elements.searchBox
  Dom.clearElement elements.resultBox

focus :: SearchElements -> Effect Unit
focus elements = Dom.focusElement elements.searchBox
