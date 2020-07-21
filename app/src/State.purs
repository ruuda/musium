-- Mindec -- Music metadata indexer
-- Copyright 2019 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module State
  ( AppState (..)
  , AppView (..)
  , Event (..)
  , State (..)
  , ViewState (..)
  , new
  , handleEvent
  , updateView
  ) where

import Control.Monad.Reader.Class (ask)
import Effect (Effect)
import Effect.Aff (Aff)
import Effect.Aff.Bus as Bus
import Effect.Aff.Bus (BusW)
import Prelude

import Dom (Element)
import Dom as Dom
import Html as Html
import Model (Album)
import AlbumListView as AlbumListView

data AppView
  = ViewLibrary
  | ViewAlbum Album

data Event
  = EventInitialize (Array Album)
  | EventSelectAlbum Album

type EventBus = BusW Event

type AppState =
  { albums :: Array Album
  , currentView :: AppView
  , postEvent :: Event -> Aff Unit
  }

type ViewState =
  { albumListView :: Element
  , albumView :: Element
  }

type State =
  { appState :: AppState
  , viewState :: ViewState
  }

newAppState :: BusW Event -> AppState
newAppState bus =
  { albums: []
  , currentView: ViewLibrary
  , postEvent: \event -> Bus.write event bus
  }

newViewState :: Effect ViewState
newViewState = Html.withElement Dom.body $ do
  albumListView <- Html.div $ do
    Html.setId "album-list-view"
    Html.addClass "active"
    ask

  albumView <- Html.div $ do
    Html.setId "album-view"
    Html.addClass "inactive"
    ask

  pure { albumListView, albumView }

new :: BusW Event -> Effect State
new bus = do
  viewState <- newViewState
  pure
    { appState: newAppState bus
    , viewState: viewState
    }

handleEvent :: Event -> AppState -> Aff AppState
handleEvent event state = case event of
  EventInitialize albums -> pure $ state { albums = albums }
  EventSelectAlbum album -> pure $ state { currentView = ViewAlbum album }

updateView :: AppState -> ViewState -> Effect ViewState
updateView appState viewState = do
  Html.withElement viewState.albumListView $ AlbumListView.renderAlbumList appState.albums
  pure viewState
