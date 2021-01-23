-- Musium -- Music playback daemon with web-based library browser
-- Copyright 2020 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module State
  ( AppState (..)
  , BrowserElements (..)
  , Elements (..)
  , handleEvent
  , new
  ) where

import Control.Monad.Error.Class (class MonadThrow, throwError)
import Control.Monad.Reader.Class (ask)
import Data.Array as Array
import Data.Int as Int
import Data.Maybe (Maybe (Just, Nothing))
import Data.Time.Duration (Milliseconds (..))
import Data.Tuple (Tuple (..))
import Effect (Effect)
import Effect.Aff (Aff, Fiber)
import Effect.Aff as Aff
import Effect.Aff.Bus (BusW)
import Effect.Aff.Bus as Bus
import Effect.Class (liftEffect)
import Effect.Class.Console as Console
import Effect.Exception (Error)
import Effect.Exception as Exception
import Foreign.Object (Object)
import Foreign.Object as Object
import Prelude

import AlbumListView (AlbumListState)
import AlbumListView as AlbumListView
import AlbumView as AlbumView
import Dom (Element)
import Dom as Dom
import Event (Event, HistoryMode)
import Event as Event
import History as History
import Html (Html)
import Html as Html
import LocalStorage as LocalStorage
import Model (Album (..), AlbumId (..), QueuedTrack (..), TrackId)
import Model as Model
import NavBar (NavBarState)
import NavBar as NavBar
import Navigation (Location)
import Navigation as Navigation
import NowPlaying as NowPlaying
import Search (SearchElements)
import Search as Search
import StatusBar (StatusBarState)
import StatusBar as StatusBar
import Time (Instant)
import Time as Time

fatal :: forall m a. MonadThrow Error m => String -> m a
fatal = Exception.error >>> throwError

type EventBus = BusW Event

type BrowserElements =
  { albumListView :: Element
  , albumListRunway :: Element
  }

type Elements =
  { libraryBrowser :: BrowserElements
  , artistBrowser :: BrowserElements
  , albumView :: Element
  , currentView :: Element
  , search :: SearchElements
  , paneLibrary :: Element
  , paneArtist :: Element
  , paneAlbum :: Element
  , paneQueue :: Element
  , paneCurrent :: Element
  , paneSearch :: Element
  }

type AppState =
  { albums :: Array Album
  , albumsById :: Object Album
  , queue :: Array QueuedTrack
  , nextQueueFetch :: Fiber Unit
  , nextProgressUpdate :: Fiber Unit
  , navBar :: NavBarState
  , statusBar :: StatusBarState
  , albumListState :: AlbumListState
    -- The index of the album at the top of the viewport.
  , albumListIndex :: Int
  , location :: Location
  , elements :: Elements
  , postEvent :: Event -> Aff Unit
  }

addBrowser :: (Event -> Aff Unit) -> Html BrowserElements
addBrowser postEvent = Html.div $ do
  Html.addClass "album-list-view"
  Html.onScroll $ Aff.launchAff_ $ postEvent $ Event.ChangeViewport
  albumListRunway <- Html.div $ ask
  albumListView <- ask
  pure $ { albumListView, albumListRunway }

setupElements :: (Event -> Aff Unit) -> Effect Elements
setupElements postEvent = Html.withElement Dom.body $ do
  { paneLibrary, libraryBrowser } <- Html.div $ do
    Html.setId "library-pane"
    Html.addClass "pane"
    paneLibrary <- ask
    libraryBrowser <- addBrowser postEvent
    pure $ { paneLibrary, libraryBrowser }

  { paneArtist, artistBrowser } <- Html.div $ do
    Html.setId "artist-pane"
    Html.addClass "pane"
    Html.addClass "inactive"
    paneArtist <- ask
    artistBrowser <- addBrowser postEvent
    pure $ { paneArtist, artistBrowser }

  { paneAlbum, albumView } <- Html.div $ do
    Html.setId "album-pane"
    Html.addClass "pane"
    Html.addClass "inactive"
    paneAlbum <- ask
    -- TODO: Does it still need to be wrapped?
    albumView <- Html.div $ do
      Html.setId "album-view"
      ask
    pure { paneAlbum, albumView }

  paneQueue <- Html.div $ do
    Html.setId "queue-pane"
    Html.addClass "pane"
    Html.addClass "inactive"
    ask

  { paneCurrent, currentView } <- Html.div $ do
    Html.setId "current-pane"
    Html.addClass "pane"
    Html.addClass "inactive"
    currentView <- Html.div $ do
      Html.addClass "current"
      ask
    NowPlaying.volumeControls
    paneCurrent <- ask
    pure $ { paneCurrent, currentView }

  { paneSearch, search } <- Html.div $ do
    Html.setId "search-pane"
    Html.addClass "pane"
    Html.addClass "inactive"
    search <- Search.new postEvent
    paneSearch <- ask
    pure $ { paneSearch, search }

  liftEffect $ Dom.onResizeWindow $ Aff.launchAff_ $ postEvent $ Event.ChangeViewport

  pure
    { libraryBrowser
    , artistBrowser
    , albumView
    , currentView
    , search
    , paneLibrary
    , paneArtist
    , paneAlbum
    , paneQueue
    , paneCurrent
    , paneSearch
    }

new :: BusW Event -> Effect AppState
new bus = do
  let postEvent event = Bus.write event bus
  navBar <- Html.withElement Dom.body $ NavBar.new postEvent
  elements <- setupElements postEvent
  statusBar <- Html.withElement Dom.body $ StatusBar.new postEvent
  never <- Aff.launchSuspendedAff Aff.never
  pure
    { albums: []
    , albumsById: Object.empty
    , queue: []
    , nextQueueFetch: never
    , nextProgressUpdate: never
    , navBar: navBar
    , statusBar: statusBar
    , albumListState: { elements: [], begin: 0, end: 0 }
    , albumListIndex: 0
    , location: Navigation.Library
    , elements: elements
    , postEvent: postEvent
    }

currentTrackId :: AppState -> Maybe TrackId
currentTrackId state = case Array.head state.queue of
  Just (QueuedTrack t) -> Just t.trackId
  Nothing              -> Nothing

getAlbum :: AlbumId -> AppState -> Maybe Album
getAlbum (AlbumId id) state = Object.lookup id state.albumsById

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
      viewportHeight <- Dom.getOffsetHeight state.elements.libraryBrowser.albumListView
      y <- Dom.getScrollTop state.elements.libraryBrowser.albumListView
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
    state.elements.libraryBrowser.albumListRunway
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
      Just current | current.track == t.trackId -> do
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

    -- Build a hash map from album id to album, so we can look them up by id.
    let
      withId album@(Album a) = let (AlbumId id) = a.id in Tuple id album
      albumsById = Object.fromFoldable $ map withId albums

    liftEffect $ do
      runway <- Html.withElement state'.elements.libraryBrowser.albumListView $ do
        Html.clear
        AlbumListView.renderAlbumListRunway $ Array.length albums

      updateAlbumList $ state'
        { albums = albums
        , albumsById = albumsById
        , elements = state'.elements
          { libraryBrowser = state'.elements.libraryBrowser { albumListRunway = runway }
          }
        }

  Event.UpdateQueue queue -> do
    statusBar' <- liftEffect $ StatusBar.updateStatusBar (Array.head queue) state.statusBar
    -- TODO: Possibly update the queue, if it is in view.

    -- TODO: Only update when the track did not change.
    liftEffect $ Html.withElement state.elements.currentView $ do
      Html.clear
      case Array.head queue of
        Nothing -> NowPlaying.nothingPlayingInfo
        Just currentTrack -> NowPlaying.nowPlayingInfo currentTrack

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

  Event.NavigateTo location@Navigation.Library mode ->
    navigateTo location mode state

  Event.NavigateTo location@Navigation.NowPlaying mode ->
    navigateTo location mode state

  Event.NavigateTo location@Navigation.Search mode -> do
    -- Clear before transition, so we transition to the clean search page.
    liftEffect $ Search.clear state.elements.search
    result <- navigateTo location mode state
    -- But focus after, because it only works when the text box is visible.
    liftEffect $ Search.focus state.elements.search
    pure result

  Event.NavigateTo location@(Navigation.Album albumId) mode -> do
    case getAlbum albumId state of
      Nothing -> fatal $ "Album " <> (show albumId) <> " does not exist."
      Just album ->
        liftEffect $ Html.withElement state.elements.albumView $ do
          Html.clear
          AlbumView.renderAlbum state.postEvent album
          -- Reset the scroll position, as we recycle the container.
          Html.setScrollTop 0.0
    navigateTo location mode state

  Event.ChangeViewport -> liftEffect $ updateAlbumList state

  Event.EnqueueTrack queuedTrack ->
    -- This is an internal update, after we enqueue a track. It allows updating
    -- the queue without having to fully fetch it, although it might trigger a
    -- new fetch.
    handleEvent
      (Event.UpdateQueue $ Array.snoc state.queue queuedTrack)
      state

navigateTo :: Navigation.Location -> HistoryMode -> AppState -> Aff AppState
navigateTo newLocation historyMode state =
  let
    getPane :: Navigation.Location -> Element
    getPane loc = case loc of
      Navigation.Library    -> state.elements.paneLibrary
      Navigation.Album _    -> state.elements.paneAlbum
      Navigation.NowPlaying -> state.elements.paneCurrent
      Navigation.Search     -> state.elements.paneSearch
    paneBefore = getPane state.location
    paneAfter = getPane newLocation
    title = case newLocation of
      Navigation.NowPlaying -> "Current"
      Navigation.Search     -> "Search"
      Navigation.Library    -> "Library"
      Navigation.Album albumId -> case getAlbum albumId state of
        Just (Album album) -> album.title <> " by " <> album.artist
        Nothing            -> "Album " <> (show albumId) <> " does not exist"
  in if newLocation == state.location then pure state else do
    case historyMode of
      Event.NoRecordHistory -> pure unit
      Event.RecordHistory -> liftEffect $
        History.pushState newLocation ("Musium: " <> title)

    liftEffect $ NavBar.selectTab newLocation state.navBar

    -- Switch the pane if we have to.
    unless (paneBefore == paneAfter) $ do
      liftEffect $ Html.withElement paneBefore $ Html.addClass "out"
      liftEffect $ Html.withElement paneAfter $ do
        Html.removeClass "inactive"
        Html.removeClass "out"
        Html.addClass "in"
      -- The css transition does not trigger if we immediately remove the "in"
      -- class, so wait a bit.
      Aff.delay $ Milliseconds (5.0)
      liftEffect $ Html.withElement paneAfter $ Html.removeClass "in"
      -- After the transition-out is complete, hide the old element entirely.
      Aff.delay $ Milliseconds (100.0)
      liftEffect $ Html.withElement paneBefore $ do
        Html.addClass "inactive"
        Html.removeClass "out"

    pure $ state { location = newLocation }

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
