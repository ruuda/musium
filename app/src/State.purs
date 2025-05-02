-- Musium -- Music playback daemon with web-based library browser
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
import Data.Array.NonEmpty as NonEmptyArray
import Data.Maybe (Maybe (Just, Nothing))
import Data.Time.Duration (Milliseconds (..))
import Data.Tuple (Tuple (..))
import Effect (Effect)
import Effect.Aff (Aff, Fiber)
import Effect.Aff as Aff
import Effect.Aff.Bus (BusW)
import Effect.Aff.Bus as Bus
import Effect.Class (liftEffect)
import Effect.Exception (Error)
import Effect.Exception as Exception
import Foreign.Object (Object)
import Foreign.Object as Object
import Prelude

import About as About
import About (AboutElements)
import AlbumListView (AlbumListView)
import AlbumListView as AlbumListView
import AlbumView (AlbumViewState)
import AlbumView as AlbumView
import Dom (Element)
import Dom as Dom
import Event (Event, HistoryMode, SearchSeq (..), SortMode, SortField (..), SortDirection (..))
import Event as Event
import History as History
import Html as Html
import Model (Artist, ArtistId, Album (..), AlbumId (..), QueueId, ScanStage (..), ScanStatus (..), QueuedTrack (..), TrackId)
import Model as Model
import NavBar (NavBarState)
import NavBar as NavBar
import Navigation (Location)
import Navigation as Navigation
import NowPlaying (NowPlayingState (..))
import NowPlaying as NowPlaying
import QueueView (QueueView)
import QueueView as QueueView
import Search (SearchElements)
import Search as Search
import StatusBar (StatusBarState)
import StatusBar as StatusBar
import Time (Instant)
import Time as Time

fatal :: forall m a. MonadThrow Error m => String -> m a
fatal = Exception.error >>> throwError

type EventBus = BusW Event

type Elements =
  { libraryBrowser :: AlbumListView
  , artistBrowser :: AlbumListView
  , albumView :: Element
  , queueView :: QueueView
  , currentView :: Element
  , search :: SearchElements
  , about :: AboutElements
  , paneLibrary :: Element
  , paneArtist :: Element
  , paneAlbum :: Element
  , paneQueue :: Element
  , paneCurrent :: Element
  , paneSearch :: Element
  , paneAbout :: Element
  }

type AppState =
  { albums :: Array Album
  , albumsById :: Object Album
  , sort :: SortMode
  , currentArtist :: Maybe Artist
  , queue :: Array QueuedTrack
  , nextQueueFetch :: Fiber Unit
  , nextProgressUpdate :: Fiber Unit
  , navBar :: NavBarState
  , statusBar :: StatusBarState
  , location :: Location
  , lastSearchRendered :: SearchSeq
  , lastArtists :: Array ArtistId
  , lastAlbum :: Maybe AlbumId
  , currentTrack :: Maybe QueueId
  , prefetchedThumb :: Maybe QueueId
  , prefetchedFull :: Maybe QueueId
  , nowPlayingState :: NowPlayingState
  , albumView :: Maybe AlbumViewState
  , elements :: Elements
  , postEvent :: Event -> Aff Unit
  }

setupElements :: (Event -> Aff Unit) -> Effect Elements
setupElements postEvent = Html.withElement Dom.body $ do
  { paneLibrary, libraryBrowser } <- Html.div $ do
    Html.setId "library-pane"
    Html.addClass "pane"
    paneLibrary <- ask
    libraryBrowser <- AlbumListView.new postEvent
    pure $ { paneLibrary, libraryBrowser }

  { paneArtist, artistBrowser } <- Html.div $ do
    Html.setId "artist-pane"
    Html.addClass "pane"
    Html.addClass "inactive"
    paneArtist <- ask
    artistBrowser <- AlbumListView.new postEvent
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

  { paneQueue, queueView } <- Html.div $ do
    Html.setId "queue-pane"
    Html.addClass "pane"
    Html.addClass "inactive"
    paneQueue <- ask
    queueView <- QueueView.new postEvent
    pure $ { paneQueue, queueView }

  { paneCurrent, currentView } <- Html.div $ do
    Html.setId "current-pane"
    Html.addClass "pane"
    Html.addClass "inactive"
    currentView <- Html.div $ do
      Html.addClass "current"
      _ <- NowPlaying.nothingPlayingInfo
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

  { paneAbout, about } <- Html.div $ do
    Html.setId "about-pane"
    Html.addClass "pane"
    Html.addClass "inactive"
    paneAbout <- ask
    about <- About.new postEvent
    pure $ { paneAbout, about }

  liftEffect $ Dom.onResizeWindow $ Aff.launchAff_ $ postEvent $ Event.ChangeViewport

  pure
    { libraryBrowser
    , artistBrowser
    , albumView
    , queueView
    , currentView
    , search
    , about
    , paneLibrary
    , paneArtist
    , paneAlbum
    , paneQueue
    , paneCurrent
    , paneSearch
    , paneAbout
    }

new :: BusW Event -> Effect AppState
new bus = do
  let postEvent event = Bus.write event bus
  navBar <- Html.withElement Dom.body $ NavBar.new postEvent
  elements <- setupElements postEvent
  statusBar <- Html.withElement Dom.body $ StatusBar.new postEvent
  never <- Aff.launchSuspendedAff Aff.never

  -- Install a handler for intercepting keyboard shortcuts.
  Dom.onWindowKeyDown $ \key ->
    if key == "/" || key == "?"
      then Aff.launchAff_ $ postEvent Event.SearchKeyPressed
      else pure unit

  pure
    { albums: []
    , albumsById: Object.empty
    , sort: { field: SortReleaseDate, direction: SortDecreasing }
    , currentArtist: Nothing
    , queue: []
    , nextQueueFetch: never
    , nextProgressUpdate: never
    , navBar: navBar
    , statusBar: statusBar
    , location: Navigation.Library
    , lastArtists: []
    , lastSearchRendered: SearchSeq 0
    , lastAlbum: Nothing
    , currentTrack: Nothing
    , prefetchedThumb: Nothing
    , prefetchedFull: Nothing
    , nowPlayingState: StateNotPlaying
    , albumView: Nothing
    , elements: elements
    , postEvent: postEvent
    }

getAlbum :: AlbumId -> AppState -> Maybe Album
getAlbum (AlbumId id) state = Object.lookup id state.albumsById

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
          let currentTrack = (QueuedTrack t)
          -- Update the progress bar in the status bar.
          delay <- liftEffect $ StatusBar.updateProgressBar currentTrack state.statusBar
          -- And the waveform in the "now playing" page. Like above it returns
          -- the delay until the next update, they should be the same, so we
          -- ignore this one.
          _ <- liftEffect $ NowPlaying.updateProgressBar currentTrack state.nowPlayingState

          -- Schedule the next update.
          fiber <- Aff.forkAff $ do
            Aff.delay $ Time.toNonNegativeMilliseconds delay
            state.postEvent $ Event.UpdateProgress

          pure $ state { nextProgressUpdate = fiber }

      _ -> fatal "Mismatch between status bar current track, and queue."

updateScanStatus :: ScanStatus -> AppState -> Aff AppState
updateScanStatus (ScanStatus newStatus) state = do
  liftEffect $ About.updateScanStatus state.elements.about (ScanStatus newStatus)

  -- If the scan is still ongoing, wait a bit and then poll again for the new
  -- status.
  case newStatus.stage of
    ScanDone -> liftEffect $ About.refreshStats state.elements.about
    -- TODO: Store the fiber in the state, so that we can cancel any currently
    -- running ones, to ensure we don't amplify the queries.
    _ -> void $ Aff.forkAff $ do
      Aff.delay $ Milliseconds 100.0
      Model.getScanStatus >>= case _ of
        Nothing -> pure unit
        Just s  -> state.postEvent $ Event.UpdateScanStatus s

  pure state

-- If there is a new track queued, and it is going to play soon, begin
-- prefetching its thumb and waveform, and possibly the cover image, if the now
-- playing pane is visible. This ensures that when the Now Playing pane is
-- visible, there is no flash of missing waveform when the next track starts.
prefetchImagesIfNeeded :: Instant -> AppState -> Effect AppState
prefetchImagesIfNeeded now state = case Array.head state.queue of
  -- We only take action if the track ends within 30 seconds.
  Just first | Model.timeLeft now first < Time.fromSeconds 30.0 ->
    case Array.index state.queue 1 of
      Nothing -> pure state
      Just (QueuedTrack track) -> do
        -- If we did not yet prefetch the thumb for the next track, do so now.
        -- This is always useful, because the thumb is always visible in the
        -- status bar (though also likely cached).
        state' <- if state.prefetchedThumb /= Just track.queueId
          then do
            _ <- Dom.createImg (Model.thumbUrl track.albumId) "Thumb preload"
            pure $ state { prefetchedThumb = Just track.queueId }
          else
            pure state

        -- Then additionally, if we are on the now playing pane, and we did
        -- not yet prefetch the next waveform or cover image, do so now.
        case state'.location of
          Navigation.NowPlaying | state.prefetchedFull /= Just track.queueId -> do
            _ <- Dom.createImg (Model.waveformUrl track.trackId) "Waveform preload"
            _ <- Dom.createImg (Model.coverUrl track.albumId) "Cover preload"
            pure $ state' { prefetchedFull = Just track.queueId }
          _ -> pure state'

  _ -> pure state

sortAlbums :: SortMode -> Array Album -> Array Album
sortAlbums {field, direction} albums =
  let
    applyDir = case direction of
      SortIncreasing -> identity
      SortDecreasing -> Array.reverse
  in
    applyDir $ case field of
      SortReleaseDate -> Array.sortWith (\(Album album) -> album.releaseDate)   albums
      SortFirstSeen   -> Array.sortWith (\(Album album) -> album.firstSeen)     albums
      SortDiscover    -> Array.sortWith (\(Album album) -> album.discoverScore) albums
      SortTrending    -> Array.sortWith (\(Album album) -> album.trendingScore) albums
      SortForNow      -> Array.sortWith (\(Album album) -> album.forNowScore)   albums

toggleSortDirection :: SortDirection -> SortDirection
toggleSortDirection = case _ of
  SortIncreasing -> SortDecreasing
  SortDecreasing -> SortIncreasing

handleEvent :: Event -> AppState -> Aff AppState
handleEvent event state = case event of
  Event.Initialize albums queue -> do
    -- Build a hash map from album id to album, so we can look them up by id.
    let
      withId album@(Album a) = let (AlbumId id) = a.id in Tuple id album
      albumsById = Object.fromFoldable $ map withId albums
      albumsSorted = sortAlbums state.sort albums

    libraryBrowser <- liftEffect $ do
      AlbumListView.setSortMode state.sort state.elements.libraryBrowser
      AlbumListView.setSortMode state.sort state.elements.artistBrowser
      AlbumListView.setAlbums albumsSorted state.elements.libraryBrowser

    let
      state' = state
        { albums = albumsSorted
        , albumsById = albumsById
        , elements = state.elements { libraryBrowser = libraryBrowser }
          -- See also below.
        , location = if Array.length queue > 0
            then Navigation.NowPlaying
            else Navigation.Library
        }

    state'' <- handleEvent (Event.UpdateQueue queue) state'


    -- If nothing is playing, then remain on the lirary page. But if something
    -- is playing, then we open the "now playing" pane initially. We don't use
    -- the regular NavigateTo event for this, to side-step the transition
    -- animation that would briefly show the library pane.
    initialPane <- if Array.length queue > 0
      then liftEffect $ do
        Html.withElement state''.elements.paneLibrary $ Html.addClass "inactive"
        Html.withElement state''.elements.paneCurrent $ Html.removeClass "inactive"
        pure state''.elements.paneCurrent
      else
        pure state''.elements.paneLibrary

    -- Apply the transition animation initially, because the page doesn't load
    -- instantly, and this is a bit nicer than having things pop in. TODO: It
    -- does not have any effect for the library pane though, that needs better
    -- timing.
    liftEffect $ Html.withElement initialPane $ do
      Html.addClass "in"
      Html.forceLayout
      Html.removeClass "in"

    -- Ensure the right tab is selected initially.
    liftEffect $ NavBar.selectInitialTab state''.location state''.navBar

    pure state''

  Event.UpdateQueue queue -> do
    statusBar' <- liftEffect $ StatusBar.updateStatusBar (Array.head queue) state.statusBar

    -- Update the number in the bubble on the queue tab. The number is one less
    -- than the length, because the queue here includes the currently playing
    -- track.
    liftEffect $ NavBar.setQueueSize state.navBar $ max 0 ((Array.length queue) - 1)

    -- Update the queue view (unconditionally for now, we could optimize this
    -- to only update it when visible, but then we need to redraw it when we
    -- switch to it).
    liftEffect $ QueueView.setQueue state.elements.queueView queue

    -- Update the "Current" / "Now Playing" page only if the current track
    -- changed. We don't want to rebuild the DOM nodes all the time if the track
    -- did not change, to be able to preserve transitions. Also, it's just
    -- easier to debug if DOM nodes don't disappear all the time.
    let currentTrack = map (\(QueuedTrack t) -> t.queueId) (Array.head queue)
    newNowPlayingState <- if state.currentTrack /= currentTrack
      then do
        -- TODO: Use a nice transition, like in the status bar.
        liftEffect $ Html.withElement state.elements.currentView $ do
          Html.clear
          case Array.head queue of
            Nothing -> NowPlaying.nothingPlayingInfo
            Just track -> NowPlaying.nowPlayingInfo state.postEvent track
      else
        pure state.nowPlayingState

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
      , currentTrack = currentTrack
      , nowPlayingState = newNowPlayingState
      }

  Event.UpdateProgress -> do
    now <- liftEffect $ Time.getCurrentInstant
    state' <- liftEffect $ prefetchImagesIfNeeded now state
    updateProgressBar state'

  Event.UpdateScanStatus newStatus -> updateScanStatus newStatus state

  Event.NavigateTo location@Navigation.Library mode ->
    navigateTo location mode state

  Event.NavigateTo location@Navigation.Queue mode ->
    navigateTo location mode state

  Event.NavigateTo location@Navigation.NowPlaying mode ->
    navigateTo location mode state

  Event.NavigateTo location@Navigation.About mode ->
    navigateTo location mode state

  Event.NavigateTo location@Navigation.Search mode -> do
    -- Clear before transition, so we transition to the clean search page.
    liftEffect $ Search.clear state.elements.search
    result <- navigateTo location mode state
    -- But focus after, because it only works when the text box is visible.
    liftEffect $ Search.focus state.elements.search
    pure result

  Event.NavigateTo location@(Navigation.Artist artistId) mode -> do
    artist <- Model.getArtist artistId
    albumView <- liftEffect $
      AlbumListView.setAlbums
        (sortAlbums state.sort artist.albums)
        state.elements.artistBrowser
    -- TODO Html.setScrollTop 0.0
    navigateTo location mode $ state
      { currentArtist = Just artist
      , elements = state.elements { artistBrowser = albumView }
      , lastArtists = [artistId]
      }

  Event.NavigateTo location@(Navigation.Album albumId) mode -> do
    { album, albumViewState } <- case getAlbum albumId state of
      Nothing -> fatal $ "Album " <> (show albumId) <> " does not exist."
      Just album -> do
        liftEffect $ Html.withElement state.elements.albumView $ do
          Html.clear
          albumViewState <- AlbumView.renderAlbumInit state.postEvent album
          -- Reset the scroll position, as we recycle the container.
          Html.setScrollTop 0.0
          pure { album, albumViewState }

          -- TODO: Now we need to wait a bit and update the album view.
    let
      Album albumDetails = album

    navigateTo location mode $ state
      { lastAlbum = Just albumId
      , lastArtists = NonEmptyArray.toArray albumDetails.artistIds
      , albumView = Just albumViewState
      }

  Event.NavigateToArtist -> case state.location of
    -- If we are already at an artist page, then lastArtists must be the artist
    -- that we are currently viewing, so it makes no sense to navigate again.
    Navigation.Artist _ -> pure state
    _notArtist ->
      let
        go target = handleEvent
          (Event.NavigateTo (Navigation.Artist target) Event.RecordHistory)
          state
      in
        case Array.head state.lastArtists of
          Just artistId -> go artistId
          -- If there is no previously visited page, but something is playing,
          -- use that instead.
          Nothing -> case Array.head state.queue of
            Just (QueuedTrack qt) -> go $ NonEmptyArray.head qt.albumArtistIds
            Nothing -> pure state

  Event.NavigateToAlbum -> case state.location of
    -- If we are already at an album page, then lastAlbum must be the album
    -- that we are currently viewing, so it makes no sense to navigate again.
    Navigation.Album _ -> pure state
    _notAlbum ->
      let
        go target = handleEvent
          (Event.NavigateTo (Navigation.Album target) Event.RecordHistory)
          state
      in
        case state.lastAlbum of
          Just albumId -> go albumId
          -- If there is no previously visited page, but something is playing,
          -- use that instead.
          Nothing -> case Array.head state.queue of
            Just (QueuedTrack qt) -> go qt.albumId
            Nothing -> pure state

  Event.ChangeViewport ->
    liftEffect $ case state.location of
      Navigation.Library -> do
        view <- AlbumListView.updateViewport state.elements.libraryBrowser
        pure $ state { elements = state.elements { libraryBrowser = view } }
      Navigation.Artist _ -> do
        view <- AlbumListView.updateViewport state.elements.artistBrowser
        pure $ state { elements = state.elements { artistBrowser = view } }
      _ -> pure state

  Event.SetSortField sortOnField ->
    let
      newSort = if state.sort.field == sortOnField
        then state.sort { direction = toggleSortDirection state.sort.direction }
        else { field: sortOnField, direction: SortDecreasing }
    in
      liftEffect $ do
        AlbumListView.setSortMode newSort state.elements.libraryBrowser
        libraryBrowser <- AlbumListView.setAlbums
          (sortAlbums newSort state.albums)
          state.elements.libraryBrowser

        AlbumListView.setSortMode newSort state.elements.artistBrowser
        artistBrowser <- AlbumListView.setAlbums
          (sortAlbums newSort state.elements.artistBrowser.albums)
          state.elements.artistBrowser

        pure $ state
          { sort = newSort
          , elements = state.elements
            { artistBrowser = artistBrowser
            , libraryBrowser = libraryBrowser
            }
          }

  Event.EnqueueTrack queuedTrack ->
    -- This is an internal update, after we enqueue a track. It allows updating
    -- the queue without having to fully fetch it, although it might trigger a
    -- new fetch.
    handleEvent
      (Event.UpdateQueue $ Array.snoc state.queue queuedTrack)
      state

  Event.ShuffleQueue -> do
    queue <- Model.shuffleQueue
    handleEvent (Event.UpdateQueue queue) state

  Event.ClearQueue -> do
    queue <- Model.clearQueue
    handleEvent (Event.UpdateQueue queue) state

  Event.SearchKeyPressed ->
    -- If we receive the search hotkey, navigate to the search pane if we aren't
    -- already there. If we are there, it could be input for the search field
    -- instead, so we need to ignore this.
    case state.location of
      Navigation.Search -> pure state
      _notSearch ->
        handleEvent (Event.NavigateTo Navigation.Search Event.RecordHistory) state

  Event.Search seq query -> do
    -- We fork a fiber to run the search query and make it post an event when
    -- it's done, we don't block the main event loop on it, so that other logic
    -- can continue even when a search is in progress, and also so that multiple
    -- search queries can run in parallel when the user is typing fast.
    _fiber <- Aff.forkAff $ do
      result <- Model.search query
      state.postEvent $ Event.SearchResult seq result
    pure state

  Event.SearchResult seq results ->
    -- We only render the search result if it is for a newer query than what
    -- we already rendered. (But it still may not be for the latest query we
    -- sent!) This ensures that slow responses get dropped, rather than
    -- overwriting later results that we already displayed.
    if seq <= state.lastSearchRendered then pure state else do
      liftEffect $ Search.renderSearchResults
        state.postEvent
        state.elements.search
        state.albumsById
        results
      pure $ state { lastSearchRendered = seq }

beforeSwitchPane :: AppState -> Aff AppState
beforeSwitchPane state =
  case state.location of
    -- When transitioning towards the album view, before we start the animation,
    -- wait a brief time for the album details to load, so we can show them
    -- immediately and animate the final page, so they don't pop in later.
    Navigation.Album albumId ->
      case state.albumView of
        Nothing -> fatal "Switching to album view without having an album view."
        Just v -> do
          let queue = getQueuedTracksForAlbum albumId state
          newAlbumViewState <- AlbumView.renderAlbumTryAdvance v queue (Milliseconds 25.0)
          pure $ state { albumView = Just newAlbumViewState }

    _notAlbum -> pure state

afterSwitchPane :: AppState -> Aff AppState
afterSwitchPane state =
  case state.location of
    -- When transitioning towards the album view, we don't allow it to load
    -- anything during the transition, because that can cause the animation to
    -- stutter. But after the transition is done, we do need to continue. We do
    -- this in a forkAff, because it should not block navigating away from the
    -- album view again.
    Navigation.Album albumId ->
      case state.albumView of
        Nothing -> fatal "Switching to album view without having an album view."
        Just v -> do
          let queue = getQueuedTracksForAlbum albumId state
          void $ Aff.forkAff $ AlbumView.renderAlbumFinalize v queue
          pure $ state { albumView = Nothing }

    _notAlbum -> pure state

navigateTo :: Navigation.Location -> HistoryMode -> AppState -> Aff AppState
navigateTo newLocation historyMode state =
  let
    getPane :: Navigation.Location -> Element
    getPane loc = case loc of
      Navigation.Library    -> state.elements.paneLibrary
      Navigation.Artist _   -> state.elements.paneArtist
      Navigation.Album _    -> state.elements.paneAlbum
      Navigation.Queue      -> state.elements.paneQueue
      Navigation.NowPlaying -> state.elements.paneCurrent
      Navigation.Search     -> state.elements.paneSearch
      Navigation.About      -> state.elements.paneAbout
    paneBefore = getPane state.location
    paneAfter = getPane newLocation
    title = case newLocation of
      Navigation.About      -> "About"
      Navigation.Library    -> "Library"
      Navigation.NowPlaying -> "Current"
      Navigation.Queue      -> "Queue"
      Navigation.Search     -> "Search"
      Navigation.Album albumId -> case getAlbum albumId state of
        Just (Album album) -> album.title <> " by " <> album.artist
        Nothing            -> "Album " <> (show albumId) <> " does not exist"
      Navigation.Artist artistId -> case state.currentArtist of
        Just artist | artist.id == artistId -> artist.name
        _ -> "Artist " <> (show artistId) <> " is not currently loaded"
  in if newLocation == state.location then pure state else do
    case historyMode of
      Event.NoRecordHistory -> pure unit
      Event.RecordHistory -> liftEffect $
        History.pushState newLocation ("Musium: " <> title)

    liftEffect $ NavBar.selectTab newLocation state.navBar

    let state' = state { location = newLocation }

    -- Switch the pane if we have to. This might change the state if we do.
    if (paneBefore == paneAfter)
      then
        pure state'
      else do
        liftEffect $ Html.withElement paneBefore $ Html.addClass "out"
        liftEffect $ Html.withElement paneAfter $ do
          Html.removeClass "inactive"
          Html.removeClass "out"
          -- Add the class to make the pane be in the "in" state, then remove it
          -- later to trigger a transition, but force layout in between so the
          -- add-remove does not become a no-op.
          Html.addClass "in"
          Html.forceLayout

        state'' <- beforeSwitchPane state'

        liftEffect $ Html.withElement paneAfter $ do
          Html.removeClass "in"

        -- After the transition-out is complete, hide the old element entirely.
        -- Add 5ms to the duration to ensure that it happens *after* the
        -- transition is complete.
        Aff.delay $ Milliseconds (105.0)

        liftEffect $ Html.withElement paneBefore $ do
          Html.addClass "inactive"
          Html.removeClass "out"

        afterSwitchPane state''

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
    state.postEvent $ Event.UpdateQueue queue

  pure $ state { nextQueueFetch = fiber }

getQueuedTracksForAlbum :: AlbumId -> AppState -> Array TrackId
getQueuedTracksForAlbum albumId state = identity
  $ map (case _ of QueuedTrack qt -> qt.trackId)
  $ Array.filter (case _ of QueuedTrack qt -> qt.albumId == albumId)
  $ state.queue
