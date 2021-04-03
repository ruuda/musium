-- Musium -- Music playback daemon with web-based library browser
-- Copyright 2020 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module AlbumView
  ( renderAlbum
  ) where

import Control.Monad.Reader.Class (ask)
import Data.Array as Array
import Data.Array.NonEmpty (NonEmptyArray)
import Data.Foldable (traverse_)
import Effect.Aff (Aff, joinFiber, launchAff, launchAff_)
import Effect.Class (liftEffect)
import Prelude

import Event (Event)
import Event as Event
import Html (Html)
import Html as Html
import Model (Album (..), QueuedTrack (..), Track (..), TrackId)
import Model as Model
import Navigation as Navigation
import Time as Time

renderAlbum :: (Event -> Aff Unit) -> Album -> Array TrackId -> Html Unit
renderAlbum postEvent (Album album) queuedTracks = do
  -- Begin loading the tracks before we add the images. The album list can be
  -- served from memory, but the cover art needs disk access. When the disks
  -- need to spin up, it can easily take a few seconds to serve the cover art,
  -- and we should't block the track list on that.
  tracksAsync <- liftEffect $ launchAff $ Model.getTracks album.id

  Html.div $ do
    Html.addClass "album-info"
    Html.div $ do
      Html.addClass "cover"
      let alt = album.title <> " by " <> album.artist
      -- Add 3 images: a blurred backdrop, the low-resolution thumbnail that
      -- should already be cached for quick display, and the high-resolution
      -- cover art on top of that.
      Html.img (Model.thumbUrl album.id) alt $ Html.addClass "backdrop"
      Html.img (Model.thumbUrl album.id) alt $ Html.addClass "lowres"
      Html.img (Model.coverUrl album.id) alt $ pure unit
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
      Html.button $ do
        Html.addClass "enqueue"
        Html.text "Enqueue"
      Html.button $ do
        Html.addClass "play-next"
        Html.text "Play Next"

  trackList <- Html.ul $ do
    Html.addClass "track-list"
    ask

  liftEffect $ launchAff_ $ do
    tracks <- joinFiber tracksAsync
    liftEffect $ Html.withElement trackList $ traverse_
      (renderDisc postEvent (Album album) queuedTracks)
      (Array.groupBy isSameDisc tracks)

isSameDisc :: Track -> Track -> Boolean
isSameDisc (Track t1) (Track t2) = t1.discNumber == t2.discNumber

renderDisc
  :: (Event -> Aff Unit)
  -> Album
  -> Array TrackId
  -> NonEmptyArray Track
  -> Html Unit
renderDisc postEvent album queuedTracks tracks = Html.div $ do
  Html.addClass "disc"
  traverse_ (renderTrack postEvent album queuedTracks) tracks

renderTrack
  :: (Event -> Aff Unit)
  -> Album
  -> Array TrackId
  -> Track
  -> Html Unit
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

    trackElement <- ask

    when (track.id `Array.elem` queuedTracks) $ Html.addClass "queued"

    Html.onClick $ do
      Html.withElement trackElement $ Html.addClass "queueing"
      launchAff_ $ do
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
