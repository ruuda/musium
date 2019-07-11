-- Mindec -- Music metadata indexer
-- Copyright 2019 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module AlbumComponent
  ( renderAlbum
  ) where

import Control.Monad.Reader.Class (ask, local)
import Data.Array as Array
import Data.Foldable (traverse_)
import Data.Maybe (Maybe (..))
import Effect.Aff (Aff, launchAff_)
import Effect.Class (liftEffect)
import Effect.Class.Console as Console
import Prelude

import Cast as Cast
import Html (Html)
import Html as Html
import Model (Album (..), Track (..))
import Model as Model
import Var as Var

renderAlbum :: Album -> Html Unit
renderAlbum (Album album) =
  Html.li $ do
    Html.addClass "album"
    header <- Html.div $ do
      Html.img (Model.thumbUrl album.id) (album.title <> " by " <> album.artist)
      Html.div $ do
        Html.addClass "album-header"
        Html.span $ do
          Html.addClass "title"
          Html.text album.title
        Html.span $ do
          Html.addClass "artist"
          Html.text album.artist
      ask

    trackList <- Html.ul $ do
      Html.addClass "track-list"
      ask

    isLoadedVar <- liftEffect $ Var.create false
    isOpenVar <- liftEffect $ Var.create false

    local (const header) $ do
      Html.onClick $ do
        let
          doOpen = do
            Var.set isOpenVar true
            Html.appendTo trackList $ do
              Html.addClass "expanded"
          doClose = do
            Var.set isOpenVar false
            Html.appendTo trackList $ do
              Html.removeClass "expanded"

        loaded <- Var.get isLoadedVar
        if loaded
          then do
            isOpen <- Var.get isOpenVar
            if isOpen then doClose else doOpen
          else do
            launchAff_ $ do
              tracks <- Model.getTracks album.id
              Console.log $ "Received tracks: " <> (show $ Array.length tracks)
              liftEffect $ do
                Var.set isLoadedVar true
                Var.set isOpenVar true
                Html.appendTo trackList $ do
                  traverse_ (renderTrack $ Album album) tracks
                  Html.addClass "expanded"

renderTrack :: Album -> Track -> Html Unit
renderTrack album (Track track) =
  Html.li $ do
    Html.addClass "track"
    Html.div $ do
      Html.addClass "track-duration"
      Html.span $ do
        Html.addClass "track-number"
        Html.text $ show track.trackNumber
      Html.span $ do
        Html.addClass "duration"
        Html.text $ Model.formatDurationSeconds track.durationSeconds
    Html.div $ do
      Html.addClass "track-header"
      Html.span $ do
        Html.addClass "title"
        Html.text track.title
      Html.span $ do
        Html.addClass "artist"
        Html.text track.artist

    trackElement <- ask

    Html.onClick $ do
      Html.appendTo trackElement $ Html.addClass "queueing"
      launchAff_ $ do
        playTrack album (Track track)
        -- TODO: Remove class after track is no longer in queue.
        -- Also change playing status. Or maybe this is the wrong
        -- place to update this.
        liftEffect $ Html.appendTo trackElement $ do
          Html.addClass "queued"
          Html.removeClass "queueing"

playTrack :: Album -> Track -> Aff Unit
playTrack (Album album) (Track track) =
  let
    queueItem = Cast.makeQueueItem
      { discNumber:  track.discNumber
      , trackNumber: track.trackNumber
      , title:       track.title
      , artist:      track.artist
      , albumTitle:  album.title
      , albumArtist: album.artist
      , releaseDate: album.date
                     -- TODO: Find a way to make urls work on the local network.
      , imageUrl:    "http://192.168.1.103:8233" <> Model.coverUrl track.id
      , trackUrl:    "http://192.168.1.103:8233" <> Model.trackUrl track.id
      }
  in do
    session <- Cast.getCastSession
    case Cast.getMediaSession session of
      -- If there is an existing media session, enqueue the track,
      -- but if there is none, play it directly.
      Just media -> do
        Cast.queueTrack media queueItem
        Console.log $ "Queued " <> track.title
      Nothing -> do
        Cast.playTrack session queueItem
        Console.log $ "Playing " <> track.title <> " immediately."
