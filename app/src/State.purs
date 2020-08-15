-- Mindec -- Music metadata indexer
-- Copyright 2020 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module State
  ( AppState (..)
  , Elements (..)
  , handleEvent
  , new
  ) where

import Control.Monad.Error.Class (class MonadThrow, throwError)
import Control.Monad.Reader.Class (ask)
import Data.Array as Array
import Data.Int as Int
import Data.Maybe (Maybe (Just, Nothing))
import Effect (Effect)
import Effect.Aff as Aff
import Effect.Aff (Aff, Fiber)
import Effect.Aff.Bus (BusW)
import Effect.Aff.Bus as Bus
import Effect.Class (liftEffect)
import Effect.Class.Console as Console
import Effect.Exception (Error)
import Effect.Exception as Exception
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
import Model as Model
import Navigation (Navigation)
import Navigation as Navigation
import StatusBar (StatusBarState)
import StatusBar as StatusBar
import Time (Instant)
import Time as Time

fatal :: forall m a. MonadThrow Error m => String -> m a
fatal = Exception.error >>> throwError

type EventBus = BusW Event

type Elements =
  { albumListView :: Element
  , albumListRunway :: Element
  , albumView :: Element
  }

type AppState =
  { albums :: Array Album
  , queue :: Array QueuedTrack
  , nextQueueFetch :: Fiber Unit
  , nextProgressUpdate :: Fiber Unit
  , statusBar :: StatusBarState
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

  Html.onScroll $ Aff.launchAff_ $ postEvent $ Event.ChangeViewport
  liftEffect $ Dom.onResizeWindow $ Aff.launchAff_ $ postEvent $ Event.ChangeViewport

  pure { albumListView, albumListRunway, albumView }

new :: BusW Event -> Effect AppState
new bus = do
  let postEvent event = Bus.write event bus
  elements <- setupElements postEvent
  statusBar <- Html.withElement Dom.body StatusBar.new
  never <- Aff.launchSuspendedAff Aff.never
  pure
    { albums: []
    , queue: []
    , nextQueueFetch: never
    , nextProgressUpdate: never
    , statusBar: statusBar
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
      Aff.launchAff_ $ state.postEvent $ Event.ChangeViewport
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

-- Update the progress bar, and schedule the next update event, if applicable.
updateProgressBar :: AppState -> Aff AppState
updateProgressBar state = do
  Aff.killFiber (Exception.error "Update cancelled in favor of new one.") state.nextProgressUpdate
  case Array.head state.queue of
    -- If these is no current track, there is no progress to update.
    Nothing -> pure state

    Just (QueuedTrack t) -> case state.statusBar.current of
      -- If there is a current track, and if it matches the one in the status
      -- bar, then we can update progress in the status bar.
      Just current | current.track == t.id -> do
          delay <- liftEffect $ StatusBar.updateProgressBar (QueuedTrack t) state.statusBar

          -- Schedule the next update.
          fiber <- Aff.forkAff $ do
            Aff.delay $ Time.toNonNegativeMilliseconds delay
            state.postEvent $ Event.UpdateProgress

          pure $ state { nextProgressUpdate = fiber }

      _ -> fatal "Mismatch between status bar current track, and queue."

handleEvent :: Event -> AppState -> Aff AppState
handleEvent event state = case event of
  Event.Initialize albums -> do
    -- Now that we have the album list, immediately start fetching the current
    -- queue, so that can happen in the background while we render the album
    -- list.
    state' <- fetchQueue state

    liftEffect $ do
      runway <- Html.withElement state'.elements.albumListView $ do
        Html.clear
        AlbumListView.renderAlbumListRunway $ Array.length albums

      updateAlbumList $ state'
        { albums = albums
        , elements = state'.elements { albumListRunway = runway }
        }

  Event.UpdateQueue queue -> do
    statusBar' <- liftEffect $ StatusBar.updateStatusBar (Array.head queue) state.statusBar
    -- TODO: Possibly update the queue, if it is in view.

    -- Update the queue again either 30 seconds from now, or at the time when
    -- we expect the current track will run out, so the point where we expect
    -- the queue to change. The 30-second interval is not really needed when we
    -- are the only client, but when multiple clients manipulate the queue, it
    -- could change without us knowing, so poll every 30 seconds to get back in
    -- sync.
    now <- liftEffect $ Time.getCurrentInstant
    let
      t30 = Time.add (Time.fromSeconds 30.0) now
      nextUpdate = case Array.head queue of
        Nothing -> t30
        Just (QueuedTrack t) -> min t30 t.refreshAt

    updateProgressBar <=< scheduleFetchQueue nextUpdate $ state
      { queue = queue
      , statusBar = statusBar'
      }

  Event.UpdateProgress -> updateProgressBar state

  Event.OpenAlbum (Album album) -> liftEffect $ do
    Html.withElement state.elements.albumView $ do
      Html.removeClass "inactive"
      Html.addClass "active"
      Html.clear
      AlbumView.renderAlbum state.postEvent (Album album)
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

  Event.EnqueueTrack queuedTrack ->
    -- This is an internal update, after we enqueue a track. It allows updating
    -- the queue without having to fully fetch it, although it might trigger a
    -- new fetch.
    handleEvent
      (Event.UpdateQueue $ Array.snoc state.queue queuedTrack)
      state

-- Schedule a new queue update at the given instant. Typically we would schedule
-- it just after we expect the current track to end.
scheduleFetchQueue :: Instant -> AppState -> Aff AppState
scheduleFetchQueue fetchAt state = do
  -- Cancel the previous fetch. If it was no longer running, this should be a
  -- no-op. If it was waiting, then now we replace it with a newer waiting
  -- fetch.
  Aff.killFiber (Exception.error "Fetch cancelled in favor of new fetch.") state.nextQueueFetch

  fiber <- Aff.forkAff $ do
    -- Wait until the desired fetch instant.
    now <- liftEffect $ Time.getCurrentInstant
    Aff.delay $ Time.toNonNegativeMilliseconds $ Time.subtract fetchAt now

    -- Then fetch, and send an event with the new queue.
    queue <- Model.getQueue
    Console.log "Loaded queue"
    state.postEvent $ Event.UpdateQueue queue

  pure $ state { nextQueueFetch = fiber }

-- Schedule a fetch queue right now.
fetchQueue :: AppState -> Aff AppState
fetchQueue state = do
  -- Cancel the previous fetch. If it was no longer running, this should be a
  -- no-op. If it was waiting, then now we replace it with a newer waiting
  -- fetch.
  Aff.killFiber (Exception.error "Fetch cancelled in favor of new fetch.") state.nextQueueFetch

  fiber <- Aff.forkAff $ do
    queue <- Model.getQueue
    Console.log "Loaded queue"
    state.postEvent $ Event.UpdateQueue queue

  pure $ state { nextQueueFetch = fiber }
