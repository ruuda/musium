-- Mindec -- Music metadata indexer
-- Copyright 2020 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module State
  ( AppState (..)
  , Elements (..)
  , new
  , handleEvent
  ) where

import Control.Monad.Reader.Class (ask)
import Data.Array as Array
import Data.Int as Int
import Effect (Effect)
import Effect.Aff (Aff, launchAff_)
import Effect.Aff.Bus (BusW)
import Effect.Aff.Bus as Bus
import Effect.Class (liftEffect)
import Effect.Class.Console as Console
import Prelude

import AlbumListView (AlbumListState)
import AlbumListView as AlbumListView
import AlbumView as AlbumView
import Dom (Element)
import Dom as Dom
import Event (Event)
import Event as Event
import History as History
import Html as Html
import Model (Album (..))
import Navigation (Location)
import Navigation as Navigation

type EventBus = BusW Event

type Elements =
  { albumListView :: Element
  , albumListRunway :: Element
  , albumView :: Element
  }

type AppState =
  { albums :: Array Album
  , albumListState :: AlbumListState
  , location :: Location
  , elements :: Elements
  , postEvent :: Event -> Aff Unit
  }

getScrollIndex :: Effect Int
getScrollIndex = do
  y <- Dom.getScrollTop Dom.body
  -- Album entries are 64 pixels tall at the moment.
  -- TODO: Find a better way to measure this.
  pure $ Int.floor $ y / 64.0

setupElements :: (Event -> Aff Unit) -> Effect Elements
setupElements postEvent = Html.withElement Dom.body $ do
  { self: albumListView, runway: albumListRunway } <- Html.div $ do
    Html.setId "album-list-view"
    Html.addClass "active"
    runway <- Html.div $ ask
    self <- ask
    pure { self, runway }

  albumView <- Html.div $ do
    Html.setId "album-view"
    Html.addClass "inactive"
    ask

  Html.onScroll $ do
    index <- getScrollIndex
    launchAff_ $ postEvent $ Event.ScrollToIndex index

  pure { albumListView, albumListRunway, albumView }

new :: BusW Event -> Effect AppState
new bus = do
  let postEvent event = Bus.write event bus
  elements <- setupElements postEvent
  pure
    { albums: []
    , albumListState: { elements: [], begin: 0, end: 0 }
    , location: Navigation.Library
    , elements: elements
    , postEvent: postEvent
    }

handleScroll :: Int -> AppState -> Effect AppState
handleScroll i state = do
  let
    headroom = 8
    albumsVisible = 15
    target =
      { begin: max 0 (i - headroom)
      , end: min (Array.length state.albums) (i + headroom + albumsVisible)
      }
  scrollState <- AlbumListView.updateAlbumList
    state.albums
    state.postEvent
    state.elements.albumListRunway
    target state.albumListState
  pure $ state { albumListState = scrollState }

handleEvent :: Event -> AppState -> Aff AppState
handleEvent event state = case event of
  Event.Initialize albums -> liftEffect $ do
    runway <- Html.withElement state.elements.albumListView $ do
      Html.clear
      AlbumListView.renderAlbumListRunway $ Array.length albums
    handleScroll 0 $ state { albums = albums, elements = state.elements { albumListRunway = runway } }

  Event.OpenAlbum (Album album) -> liftEffect $ do
    Html.withElement state.elements.albumView $ do
      Html.removeClass "inactive"
      Html.addClass "active"
      Html.clear
      AlbumView.renderAlbum (Album album)
    Html.withElement state.elements.albumListView $ do
      Html.removeClass "active"
      Html.addClass "inactive"
    let location = Navigation.Album (Album album)
    History.pushState location (album.title <> " by " <> album.artist) ("/album/" <> show album.id)
    pure $ state { location = location }

  Event.OpenLibrary -> liftEffect $ do
    Html.withElement state.elements.albumView $ do
      Html.removeClass "active"
      Html.addClass "inactive"
    Html.withElement state.elements.albumListView $ do
      Html.removeClass "inactive"
      Html.addClass "active"
    _index <- getScrollIndex
    pure $ state { location = Navigation.Library }

  Event.ScrollToIndex i -> case state.location of
    -- When scrolling, only update the album list if it is actually visible.
    Navigation.Library -> liftEffect $ handleScroll i state
    _ -> pure state
