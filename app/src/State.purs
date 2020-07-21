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
import Effect (Effect)
import Effect.Aff (Aff)
import Effect.Aff.Bus (BusW)
import Effect.Aff.Bus as Bus
import Effect.Class (liftEffect)
import Prelude

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
  , albumView :: Element
  }

type AppState =
  { albums :: Array Album
  , location :: Location
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
    , location: Navigation.Library
    , elements: elements
    , postEvent: \event -> Bus.write event bus
    }

handleEvent :: Event -> AppState -> Aff AppState
handleEvent event state = case event of
  Event.Initialize albums -> liftEffect $ do
    Html.withElement state.elements.albumListView $ do
      Html.clear
      AlbumListView.renderAlbumList state.postEvent albums
    pure $ state { albums = albums }

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
