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

data AppView
  = ViewLibrary
  | ViewAlbum Album

data Event
  = EventInitialize
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

newAppState :: BusW Event -> Array Album -> AppState
newAppState bus albums =
  { albums: albums
  , currentView: ViewLibrary
  , postEvent: \event -> Bus.write event bus
  }

newViewState :: Effect ViewState
newViewState = Html.withElement Dom.body $ do
  albumListView <- Html.div $ do
    Html.setId "album-list-view"
    ask

  albumView <- Html.div $ do
    Html.setId "album-view"
    ask

  pure { albumListView, albumView }

new :: BusW Event -> Array Album -> Effect State
new bus albums = do
  viewState <- newViewState
  pure
    { appState: newAppState bus albums
    , viewState: viewState
    }

handleEvent :: Event -> AppState -> AppState
handleEvent event state = case event of
  EventInitialize -> state
  EventSelectAlbum album -> state { currentView = ViewAlbum album }

updateView :: AppState -> ViewState -> Effect ViewState
updateView appState viewState = pure viewState
