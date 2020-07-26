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
import Data.Maybe (Maybe (Just, Nothing))
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
import Navigation (Navigation)
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
  , albumListEntriesPerViewport :: Int
  , navigation :: Navigation
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

  Html.onScroll $ launchAff_ $ postEvent $ Event.Scroll
  liftEffect $ Dom.onResizeWindow $ launchAff_ $ postEvent $ Event.ResizeViewport

  pure { albumListView, albumListRunway, albumView }

new :: BusW Event -> Effect AppState
new bus = do
  let postEvent event = Bus.write event bus
  elements <- setupElements postEvent
  pure
    { albums: []
    , albumListState: { elements: [], begin: 0, end: 0 }
    , albumListEntriesPerViewport: 1
    , navigation: { location: Navigation.Library, albumListIndex: 0 }
    , elements: elements
    , postEvent: postEvent
    }

-- Bring the album list in sync with the target state (the album list index and
-- the number of entries per viewport).
updateAlbumList :: AppState -> Effect AppState
updateAlbumList state =
  let
    headroom = 20
    albumsVisible = state.albumListEntriesPerViewport
    i = state.navigation.albumListIndex
    target =
      { begin: max 0 (i - headroom)
      , end: min (Array.length state.albums) (i + headroom + albumsVisible)
      }
 in do
  scrollState <- AlbumListView.updateAlbumList
    state.albums
    state.postEvent
    state.elements.albumListRunway
    target
    state.albumListState
  pure $ state { albumListState = scrollState }

handleEvent :: Event -> AppState -> Aff AppState
handleEvent event state = case event of
  Event.Initialize albums -> liftEffect $ do
    runway <- Html.withElement state.elements.albumListView $ do
      Html.clear
      AlbumListView.renderAlbumListRunway $ Array.length albums
    updateAlbumList $ state
      { albums = albums
      , elements = state.elements { albumListRunway = runway }
      , albumListEntriesPerViewport = 1
      }

  Event.OpenAlbum (Album album) -> liftEffect $ do
    Html.withElement state.elements.albumView $ do
      Html.removeClass "inactive"
      Html.addClass "active"
      Html.clear
      AlbumView.renderAlbum (Album album)
    Html.withElement state.elements.albumListView $ do
      Html.removeClass "active"
      Html.addClass "inactive"
    let navigation = state.navigation { location = Navigation.Album (Album album) }
    History.pushState
      navigation.location
      (album.title <> " by " <> album.artist)
      ("/album/" <> show album.id)
    pure $ state { navigation = navigation }

  Event.OpenLibrary -> liftEffect $ do
    Html.withElement state.elements.albumView $ do
      Html.removeClass "active"
      Html.addClass "inactive"
    Html.withElement state.elements.albumListView $ do
      Html.removeClass "inactive"
      Html.addClass "active"
    pure $ state { navigation = state.navigation { location = Navigation.Library } }

  Event.Scroll -> case state.navigation.location of
    -- When scrolling, only update the album list index if the album list is
    -- actually visible.
    Navigation.Library ->
      -- We can only determine the visible index if we know the height of an
      -- entry.
      case Array.head state.albumListState.elements of
        Nothing -> pure state
        Just elem -> liftEffect $ do
          entryHeight <- Dom.getOffsetHeight elem
          y <- Dom.getScrollTop Dom.body
          Console.log $ "Scrolled to " <> (show y)
          updateAlbumList $ state
            { navigation = state.navigation
              { albumListIndex = Int.floor $ y / entryHeight
              }
            }
    _ ->
      pure state

  Event.ResizeViewport -> case Array.head state.albumListState.elements of
    -- If we are not rendering any album list entries, then we can't measure how
    -- many album list entries should fit on the screen.
    Nothing -> pure state
    Just elem -> liftEffect $ do
      entryHeight <- Dom.getOffsetHeight elem
      viewportHeight <- Dom.getWindowHeight
      let entriesPerViewport = Int.ceil $ viewportHeight / entryHeight
      Console.log $ "Resized to " <> (show entriesPerViewport)
      updateAlbumList $ state { albumListEntriesPerViewport = entriesPerViewport }
