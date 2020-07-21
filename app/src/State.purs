-- Mindec -- Music metadata indexer
-- Copyright 2020 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module State
  ( AppState (..)
  , AppView (..)
  , State (..)
  , ViewState (..)
  , new
  , handleEvent
  , updateView
  ) where

import Control.Monad.Reader.Class (ask)
import Effect (Effect)
import Effect.Aff (Aff)
import Effect.Aff.Bus (BusW)
import Effect.Aff.Bus as Bus
import Effect.Class.Console as Console
import Prelude

import Dom (Element)
import Event as Event
import Event (Event)
import Dom as Dom
import Html as Html
import Model (Album)
import AlbumListView as AlbumListView

data AppView
  = ViewLibrary
  | ViewAlbum Album

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
  Event.Initialize albums -> pure $ state { albums = albums }
  Event.SelectAlbum album -> do
    Console.log "Selected an album"
    -- History.pushState (Just album) (album.title <> " by " <> album.artist) ("/album/" <> show album.id)
    pure $ state { currentView = ViewAlbum album }

updateView :: AppState -> ViewState -> Effect ViewState
updateView appState viewState = do
  Html.withElement viewState.albumListView $ AlbumListView.renderAlbumList appState.postEvent appState.albums
  pure viewState
  -- Html.withElement Dom.body $ AlbumView.renderAlbum $ Album album
