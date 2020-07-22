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
    -- TODO: Get scroll pos.
    launchAff_ $ postEvent $ Event.ScrollToIndex 100

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

handleEvent :: Event -> AppState -> Aff AppState
handleEvent event state = case event of
  Event.Initialize albums -> liftEffect $ do
    runway <- Html.withElement state.elements.albumListView $ do
      Html.clear
      AlbumListView.renderAlbumListRunway $ Array.length albums

    let target = { begin: 5, end: 10 }
    scrollState <- AlbumListView.updateAlbumList albums state.postEvent runway target state.albumListState
    pure $ state { albums = albums, albumListState = scrollState }

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
    pure state

  Event.ScrollToIndex i -> liftEffect $ do
    let
      target =
        { begin: i
        , end: i + state.albumListState.end - state.albumListState.begin
        }
    scrollState <- AlbumListView.updateAlbumList state.albums state.postEvent state.elements.albumListRunway target state.albumListState
    pure $ state { albumListState = scrollState }
