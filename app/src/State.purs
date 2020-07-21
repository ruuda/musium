-- Mindec -- Music metadata indexer
-- Copyright 2020 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module State
  ( AppState (..)
  , Elements (..)
  , NavState (..)
  , new
  , handleEvent
  ) where

import Control.Monad.Reader.Class (ask)
import Effect (Effect)
import Effect.Aff (Aff)
import Effect.Aff.Bus (BusW)
import Effect.Aff.Bus as Bus
import Effect.Class (liftEffect)
import Effect.Class.Console as Console
import Prelude

import Dom (Element)
import Event as Event
import Event (Event)
import Dom as Dom
import Html as Html
import Model (Album)
import AlbumListView as AlbumListView

type EventBus = BusW Event

data NavState
  = ViewLibrary
  | ViewAlbum Album

type Elements =
  { albumListView :: Element
  , albumView :: Element
  }

type AppState =
  { albums :: Array Album
  , nav :: NavState
  , elements :: Elements
  , postEvent :: Event -> Aff Unit
  }

setupElements :: Effect Elements
setupElements = Html.withElement Dom.body $ do
  albumListView <- Html.div $ do
    Html.setId "album-list-view"
    Html.addClass "active"
    ask

  albumView <- Html.div $ do
    Html.setId "album-view"
    Html.addClass "inactive"
    ask

  pure { albumListView, albumView }

new :: BusW Event -> Effect AppState
new bus = do
  elements <- setupElements
  pure
    { albums: []
    , nav: ViewLibrary
    , elements: elements
    , postEvent: \event -> Bus.write event bus
    }

handleEvent :: Event -> AppState -> Aff AppState
handleEvent event state = case event of
  Event.Initialize albums -> do
    liftEffect $ Html.withElement state.elements.albumListView $ do
      Html.clear
      AlbumListView.renderAlbumList state.postEvent albums
    pure $ state { albums = albums }

  Event.SelectAlbum album -> do
    Console.log "Selected an album"
    -- TODO: Render album
    -- History.pushState (Just album) (album.title <> " by " <> album.artist) ("/album/" <> show album.id)
    pure $ state { nav = ViewAlbum album }
