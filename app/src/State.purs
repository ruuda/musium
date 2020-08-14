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
import LocalStorage as LocalStorage
import Model (Album (..), QueuedTrack (..), TrackId)
import Navigation (Navigation)
import Navigation as Navigation
import StatusBar as StatusBar

type EventBus = BusW Event

type Elements =
  { albumListView :: Element
  , albumListRunway :: Element
  , albumView :: Element
  , statusBar :: Element
  }

type AppState =
  { albums :: Array Album
  , queue :: Array QueuedTrack
  , albumListState :: AlbumListState
    -- The index of the album at the top of the viewport.
  , albumListIndex :: Int
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

  statusBar <- Html.div $ do
    Html.setId "statusbar"
    ask

  Html.onScroll $ launchAff_ $ postEvent $ Event.ChangeViewport
  liftEffect $ Dom.onResizeWindow $ launchAff_ $ postEvent $ Event.ChangeViewport

  pure { albumListView, albumListRunway, albumView, statusBar }

new :: BusW Event -> Effect AppState
new bus = do
  let postEvent event = Bus.write event bus
  elements <- setupElements postEvent
  pure
    { albums: []
    , queue: []
    , albumListState: { elements: [], begin: 0, end: 0 }
    , albumListIndex: 0
    , navigation: { location: Navigation.Library }
    , elements: elements
    , postEvent: postEvent
    }

currentTrackId :: AppState -> Maybe TrackId
currentTrackId state = case Array.head state.queue of
  Just (QueuedTrack t) -> Just t.id
  Nothing              -> Nothing

-- Bring the album list in sync with the viewport (the album list index and
-- the number of entries per viewport).
updateAlbumList :: AppState -> Effect AppState
updateAlbumList state = do
  -- To determine a good target, we need to know how tall an entry is, so we
  -- need to have at least one already. If we don't, then we take a slice of
  -- a single item to start with, and enqueue an event to update again after
  -- this update.
  { target, index } <- case Array.head state.albumListState.elements of
    Nothing -> do
      launchAff_ $ state.postEvent $ Event.ChangeViewport
      pure $ { target: { begin: 0, end: min 1 (Array.length state.albums) }, index: 0 }
    Just elem -> do
      entryHeight <- Dom.getOffsetHeight elem
      viewportHeight <- Dom.getWindowHeight
      y <- Dom.getScrollTop Dom.body
      let
        headroom = 20
        i = Int.floor $ y / entryHeight
        albumsVisible = Int.ceil $ viewportHeight / entryHeight
      pure $
        { target:
          { begin: max 0 (i - headroom)
          , end: min (Array.length state.albums) (i + headroom + albumsVisible)
          }
        , index: i
        }
  LocalStorage.set "albumListIndex" index
  scrollState <- AlbumListView.updateAlbumList
    state.albums
    state.postEvent
    state.elements.albumListRunway
    target
    state.albumListState
  pure $ state { albumListState = scrollState, albumListIndex = index }

-- Update the status bar elements, if the current track has changed. This only
-- updates the view, it does not change the queue in the state.
updateStatusBar :: Maybe QueuedTrack -> AppState -> Effect Unit
updateStatusBar currentTrack state = do
  case currentTrack of
    -- When the current track did not change, do not re-render the status bar.
    Just (QueuedTrack t) | Just t.id == currentTrackId state -> pure unit
    Nothing | Array.null state.queue -> pure unit

    -- When it did change, clear the current status bar, and place the new one.
    Nothing -> Html.withElement state.elements.statusBar $ do
      Html.clear
    Just t  -> Html.withElement state.elements.statusBar $ do
      Html.clear
      StatusBar.renderStatusBar t

handleEvent :: Event -> AppState -> Aff AppState
handleEvent event state = case event of
  Event.Initialize albums -> liftEffect $ do
    runway <- Html.withElement state.elements.albumListView $ do
      Html.clear
      AlbumListView.renderAlbumListRunway $ Array.length albums
    updateAlbumList $ state
      { albums = albums
      , elements = state.elements { albumListRunway = runway }
      }

  Event.UpdateQueue queue -> liftEffect $ do
    updateStatusBar (Array.head queue) state
    -- TODO: Possibly update the queue, if it is in view.
    pure $ state { queue = queue }

  Event.OpenAlbum (Album album) -> liftEffect $ do
    Html.withElement state.elements.albumView $ do
      Html.removeClass "inactive"
      Html.addClass "active"
      Html.clear
      AlbumView.renderAlbum (Album album)
      Html.scrollIntoView
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

    -- Restore the scroll position.
    case Array.index
      state.albumListState.elements
      (state.albumListIndex - state.albumListState.begin)
      of
        Just element -> liftEffect $ Html.withElement element $ Html.scrollIntoView
        Nothing -> pure unit

    pure $ state { navigation = state.navigation { location = Navigation.Library } }

  Event.ChangeViewport -> case state.navigation.location of
    -- When scrolling or resizing, only update the album list when it is
    -- actually visible.
    Navigation.Library -> liftEffect $ updateAlbumList state
    _ -> pure state
