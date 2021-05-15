-- Musium -- Music playback daemon with web-based library browser
-- Copyright 2020 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module AlbumView
  ( AlbumViewState
  , AlbumViewElements
  , AlbumViewRenderState
  , renderAlbumInit
  , renderAlbumAdvance
  ) where

import Control.Monad.Reader.Class (ask)
import Data.Array as Array
import Data.Array.NonEmpty (NonEmptyArray)
import Data.Array.NonEmpty as NonEmptyArray
import Data.Time.Duration (Milliseconds (..))
import Data.Traversable (traverse, for_)
import Effect (Effect)
import Effect.Aff as Aff
import Effect.Aff (Aff, Fiber, launchAff, launchAff_)
import Effect.Class (liftEffect)
import Prelude

import Dom as Dom
import Dom (Element)
import Event (Event)
import Event as Event
import Html (Html)
import Html as Html
import Model (Album (..), QueuedTrack (..), Track (..), TrackId)
import Model as Model
import Navigation as Navigation
import Time as Time

-- Rendering the album view is somewhat complex because of a few reasons:
--
-- * We need to load the track list before we can render it, which is a fast
--   network request (the data can be served from memory), but still takes on
--   the order of ~15 ms over wifi.
-- * We need to load the full-resolution cover art, which is a potentially very
--   slow network request (might require the disk to spin up), plus potentially
--   a very CPU-intensive operation to decode and paint for large cover art.
-- * While all of this is happening, we want a smooth transition to the album
--   view pane, and if there is a layout change, or worse, an image decode and
--   repaint, during this transition, it will stutter. We should therefore avoid
--   modifying the album view while it is transitioning.
--
-- To accomodate this, we split loading into three phases:
--
-- 1. Kick off the two requests, and render everything we can at this point.
-- 2. Wait very briefly and render anything that came in during that time, then
--    start the transition. If things are still loading, start the transition
--    without the track list and cover art. (A low-res cover art should be
--    present, so this should not be very noticeable.)
-- 3. Render the remaining things as soon as they are available.

type AlbumViewElements =
  { trackList :: Element
  , albumActions :: Element
  , cover :: Element
  }

data AlbumViewRenderState
  -- Both the track list and full-res cover <img> are loading.
  = AllPending (Fiber (Array Track)) Element
  -- The track list has been rendered, but the cover is still loading.
  | CoverPending Element
  -- Both the track list and cover have been rendered.
  | Done

type AlbumViewState =
  { postEvent :: Event -> Aff Unit
  , album :: Album
  , elements :: AlbumViewElements
  , renderState :: AlbumViewRenderState
  }

-- Render as much of the album view as possible already, and kick off external
-- requests that may take some time to load.
renderAlbumInit
  :: (Event -> Aff Unit)
  -> Album
  -> Html AlbumViewState
renderAlbumInit postEvent (Album album) = do
  -- Begin loading the tracks before we add the images. The album list can be
  -- served from memory, but the cover art needs disk access. When the disks
  -- need to spin up, it can easily take a few seconds to serve the cover art,
  -- and we should't block the track list on that.
  tracksAsync <- liftEffect $ launchAff $ Model.getTracks album.id

  -- Continue building the basic structure of the album view.
  { coverElement, albumActionsElement, imgFullRes } <- Html.div $ do
    Html.addClass "album-info"

    { coverElement, imgFullRes } <- Html.div $ do
      Html.addClass "cover"
      let alt = album.title <> " by " <> album.artist
      -- Add 3 images: a blurred backdrop, the low-resolution thumbnail that
      -- should already be cached for quick display, and the high-resolution
      -- cover art on top of that.
      Html.img (Model.thumbUrl album.id) alt $ Html.addClass "backdrop"
      Html.img (Model.thumbUrl album.id) alt $ Html.addClass "lowres"

      -- The full-res image we don't add as a child node directly though,
      -- because that leaves us no control of when it gets painted. It might
      -- get painted during the transition for opening the album view, and in
      -- Chrome that makes the animation stutter. So we create the node, but
      -- only add it later at a controlled time.
      imgFullRes <- liftEffect $ Dom.createImg (Model.coverUrl album.id) alt
      liftEffect $ Html.withElement imgFullRes $ Html.addClass "fullres"

      coverElement <- ask
      pure { coverElement, imgFullRes }

    Html.hgroup $ do
      Html.h1 $ Html.text album.title
      Html.h2 $ do
        Html.span $ do
          Html.addClass "artist"
          Html.text album.artist
          Html.onClick $ launchAff_ $
            postEvent $ Event.NavigateTo (Navigation.Artist album.artistId) Event.RecordHistory
        Html.text " ⋅ "
        Html.span $ do
          Html.addClass "date"
          Html.text album.date

    Html.div $ do
      Html.addClass "album-actions"
      albumActionsElement <- ask
      -- This will be filled later once the track list is available.
      pure { coverElement, albumActionsElement, imgFullRes }

  trackListElement <- Html.ul $ do
    Html.addClass "track-list"
    ask
    -- This will be filled once the track list is available.

  pure $
    { postEvent: postEvent
    , album: Album album
    , elements:
      { trackList: trackListElement
      , albumActions: albumActionsElement
      , cover: coverElement
      }
    , renderState: AllPending tracksAsync imgFullRes
    }

renderAlbumAdvance
  :: AlbumViewState
  -> Array TrackId
  -> Aff AlbumViewState
renderAlbumAdvance state queuedTracks = case state.renderState of
  AllPending tracksAsync img -> do
    tracks <- Aff.joinFiber tracksAsync
    liftEffect $ renderTrackList state queuedTracks tracks

    -- If at this point the cover is loaded, include it immediately.
    isCoverLoaded <- liftEffect $ Dom.getComplete img
    if isCoverLoaded
      then liftEffect $ insertFullResCover state img
      else pure $ state { renderState = CoverPending img }

  CoverPending img -> do
    Dom.waitComplete img
    liftEffect $ insertFullResCover state img

  Done ->
    pure state

insertFullResCover :: AlbumViewState -> Element -> Effect AlbumViewState
insertFullResCover state img =
  Html.withElement state.elements.cover $
    Html.element img $
      pure $ state { renderState = Done }

renderTrackList
  :: AlbumViewState
  -> Array TrackId
  -> Array Track
  -> Effect Unit
renderTrackList state queuedTracks tracks = do
    -- Group the tracks by disk and render one <div> per disc, so we can leave
    -- some space in between. Collects the track <li> elements as an array per
    -- disc.
    discStates <- Html.withElement state.elements.trackList $ traverse
      (renderDisc state.postEvent state.album queuedTracks)
      (Array.groupBy isSameDisc tracks)

    Html.withElement state.elements.albumActions $ do
      -- For an album with a single disc, we just show an "enqueue" button,
      -- but if we have multiple discs, show one per disc, and label them
      -- appropriately.
      let
        label discState = case Array.length discStates of
          1 -> "Enqueue"
          _ -> "Enqueue Disc " <> (show discState.number)

      for_ discStates $ \discState -> Html.button $ do
        Html.addClass "enqueue"
        Html.text $ label discState
        -- When we enqueue the album, simply enqueue all tracks individually.
        -- Because enqueueTrack returns an Aff, this will not enqueue a track
        -- before the previous one is confirmed enqueued. However, we still add
        -- a little sleep in between, to have a nice visual effect of the tracks
        -- being enqueued one by one.
        Html.onClick $ launchAff_ $
          for_ discState.tracks $ \t -> do
            enqueueTrack state.postEvent state.album t.track t.element
            Aff.delay $ Milliseconds (25.0)

      Html.button $ do
        Html.addClass "play-next"
        Html.text "Play Next"
        -- TODO: Do we need a "play next" functionality at all?

isSameDisc :: Track -> Track -> Boolean
isSameDisc (Track t1) (Track t2) = t1.discNumber == t2.discNumber

type DiscState =
  { number :: Int
  , tracks :: NonEmptyArray { track :: Track, element :: Element }
  }

renderDisc
  :: (Event -> Aff Unit)
  -> Album
  -> Array TrackId
  -> NonEmptyArray Track
  -> Html DiscState
renderDisc postEvent album queuedTracks tracks = Html.div $ do
  Html.addClass "disc"
  elements <- traverse (renderTrack postEvent album queuedTracks) tracks
  let Track firstTrack = NonEmptyArray.head tracks
  pure
    { number: firstTrack.discNumber
    , tracks: NonEmptyArray.zipWith (\t e -> { track: t, element: e }) tracks elements
    }

enqueueTrack
  :: (Event -> Aff Unit)
  -> Album
  -> Track
  -> Element
  -> Aff Unit
enqueueTrack postEvent (Album album) (Track track) trackElement = do
  liftEffect $ Html.withElement trackElement $ Html.addClass "queueing"
  queueId <- Model.enqueueTrack $ track.id
  now <- liftEffect $ Time.getCurrentInstant
  postEvent $ Event.EnqueueTrack $ QueuedTrack
    { queueId: queueId
    , trackId: track.id
    , title: track.title
    , artist: track.artist
    , album: album.title
    , albumId: album.id
    , albumArtistId: album.artistId
    , durationSeconds: track.durationSeconds
    , positionSeconds: 0.0
    , bufferedSeconds: 0.0
      -- Assume not buffering when we add the track, to avoid showing the
      -- spinner in the happy case where playback starts instantly. In the
      -- unhappy case where buffering takes a long time, the thumbnail
      -- will dim later to reveal the spinner.
    , isBuffering: false
    , startedAt: now
      -- Add a small delay before we refresh. If the queue was empty and
      -- the enqueue triggered the track, the server should focus on
      -- playing and establishing a safe buffer first, before we bother it
      -- with queue status requests. Also give it enough headroom that it
      -- should not have an empty buffer by the time we poll again, to
      -- prevent the spinner from showing up.
    , refreshAt: Time.add (Time.fromSeconds 0.4) now
    }
  -- TODO: Remove class after track is no longer in queue.
  liftEffect $ Html.withElement trackElement $ do
    Html.addClass "queued"
    Html.removeClass "queueing"

-- Render a track <li>. Returns the element itself, so it its queueing indicator
-- can be modified later.
renderTrack
  :: (Event -> Aff Unit)
  -> Album
  -> Array TrackId
  -> Track
  -> Html Element
renderTrack postEvent (Album album) queuedTracks (Track track) =
  Html.li $ do
    Html.addClass "track"

    Html.div $ do
      Html.addClass "track-number"
      Html.text $ show track.trackNumber
    Html.div $ do
      Html.addClass "title"
      Html.text track.title
    Html.div $ do
      Html.addClass "duration"
      Html.text $ Model.formatDurationSeconds track.durationSeconds
    Html.div $ do
      Html.addClass "artist"
      Html.text track.artist

    when (track.id `Array.elem` queuedTracks) $ Html.addClass "queued"

    trackElement <- ask
    Html.onClick $ launchAff_ $ enqueueTrack postEvent (Album album) (Track track) trackElement

    pure trackElement
