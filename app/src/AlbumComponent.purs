-- Mindec -- Music metadata indexer
-- Copyright 2019 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module AlbumComponent
  ( renderAlbum'
  ) where

import Control.Monad.Reader.Class (ask, local)
import Data.Array as Array
import Data.Foldable (traverse_)
import Effect (Effect)
import Effect.Aff (launchAff_)
import Effect.Class (liftEffect)
import Effect.Class.Console as Console
import Prelude

import Cast as Cast
import Html (Html)
import Html as Html
import Model (Album (..), Track (..))
import Model as Model
import Var as Var

renderAlbum' :: Album -> Html Unit
renderAlbum' (Album album) =
  Html.li $ do
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
      Html.addClass "collapsed"
      ask

    isLoadedVar <- liftEffect $ Var.create false
    isOpenVar <- liftEffect $ Var.create false

    local (const header) $ do
      Html.onClick $ do
        let
          doOpen = do
            Var.set isOpenVar true
            Html.appendTo trackList $ do
              Html.removeClass "collapsed"
              Html.addClass "expanded"
          doClose = do
            Var.set isOpenVar false
            Html.appendTo trackList $ do
              Html.removeClass "expanded"
              Html.addClass "collapsed"

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
                  Html.removeClass "collapsed"
                  Html.addClass "expanded"
                  traverse_ (renderTrack' $ Album album) tracks

renderTrack' :: Album -> Track -> Html Unit
renderTrack' album (Track track) =
  Html.li $ do
    Html.div $ do
      Html.addClass "track-duration"
      Html.span $ do
        Html.addClass "track"
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

    Html.onClick $ playTrack album (Track track)

playTrack :: Album -> Track -> Effect Unit
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
  in
    Cast.queueTrack queueItem
