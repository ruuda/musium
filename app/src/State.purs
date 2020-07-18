-- Mindec -- Music metadata indexer
-- Copyright 2019 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module State
  ( AppState (..)
  , AppView (..)
  , State
  , ViewState (..)
  , new
  ) where

import Control.Monad.Reader.Class (ask)
import Effect (Effect)
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

type AppState =
  { albums :: Array Album
  , currentView :: AppView
  }

type ViewState =
  { albumListView :: Element
  , albumView :: Element
  }

type State =
  { appState :: AppState
  , viewState :: ViewState
  }

newAppState :: Array Album -> AppState
newAppState albums =
  { albums: albums
  , currentView: ViewLibrary
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

new :: Array Album -> Effect State
new albums = do
  viewState <- newViewState
  pure
    { appState: newAppState albums
    , viewState: viewState
    }

handleEvent :: Event -> AppState -> AppState
handleEvent event state = case event of
  EventInitialize -> state
  EventSelectAlbum album -> state { currentView = ViewAlbum album }

updateView :: AppState -> ViewState -> Effect ViewState
updateView appState viewState = pure viewState
