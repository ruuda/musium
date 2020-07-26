-- Mindec -- Music metadata indexer
-- Copyright 2020 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module AlbumView
  ( renderAlbum
  ) where

import Control.Monad.Reader.Class (ask)
import Data.Foldable (traverse_)
import Effect.Aff (joinFiber, launchAff, launchAff_)
import Effect.Class (liftEffect)
import Prelude

import Html (Html)
import Html as Html
import Model (Album (..), Track (..))
import Model as Model

renderAlbum :: Album -> Html Unit
renderAlbum (Album album) = do
  -- Begin loading the tracks before we add the images. The current server is
  -- single-threaded, and the album list can be served from memory, but the
  -- cover art needs disk access. When the disks need to spin up, it can easily
  -- take a few seconds to serve the cover art, and we should't block the track
  -- list on that.
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
      Html.img (Model.thumbUrl album.id) alt $ pure unit
      Html.img (Model.coverUrl album.id) alt $ pure unit
    Html.hgroup $ do
      Html.h1 $ Html.text album.title
      Html.h2 $ do
        Html.span $ do
          Html.addClass "artist"
          Html.text album.artist
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
    liftEffect
      $ Html.withElement trackList
      $ traverse_ (renderTrack $ Album album) tracks

renderTrack :: Album -> Track -> Html Unit
renderTrack album (Track track) =
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
