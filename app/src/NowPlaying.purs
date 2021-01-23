-- Musium -- Music playback daemon with web-based library browser
-- Copyright 2020 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module NowPlaying
  ( nothingPlayingInfo
  , nowPlayingInfo
  , volumeControls
  ) where

import Control.Monad.Reader.Class (ask)
import Effect.Aff (Aff, launchAff_)
import Effect.Class (liftEffect)
import Prelude

import Dom as Dom
import Event (Event)
import Event as Event
import Navigation as Navigation
import Html (Html)
import Html as Html
import Model (Decibel (Decibel), QueuedTrack (QueuedTrack), Volume (Volume))
import Model as Model

volumeControls :: Html Unit
volumeControls = Html.div $ do
  Html.addClass "volume-controls"
  { volumeBar, label } <- Html.div $ do
    Html.addClass "indicator"
    Html.div $ do
      label <- Html.div $ do
        Html.addClass "volume-label"
        ask
      volumeBar <- ask
      pure $ { volumeBar, label }

  -- Now that we have the elements that display the current volume,
  -- define a function that alters those elements to display a certain volume.
  let
    setVolume (Decibel v) = do
      -- Use -20 dB as the minimum of the bar and 10 dB as the maximum.
      -- Set the minimum width to 1% so you can see that there is something that
      -- fills the volume bar.
      let percentage = max 1.0 $ min 100.0 $ (v + 20.0) / 0.3
      Dom.setWidth (show percentage <> "%") volumeBar
      Html.withElement label $ do
        Html.clear
        Html.text $ show v <> " dB"

    changeVolume dir = liftEffect $ launchAff_ $ do
      Volume newVolume <- Model.changeVolume dir
      liftEffect $ setVolume newVolume.volume

  -- Fetch the initial volume.
  liftEffect $ launchAff_ $ do
    Volume v <- Model.getVolume
    liftEffect $ setVolume v.volume

  Html.button $ do
    Html.addClass "volume-down"
    Html.text "V-"
    Html.onClick $ changeVolume Model.VolumeDown
  Html.button $ do
    Html.addClass "volume-up"
    Html.text "V+"
    Html.onClick $ changeVolume Model.VolumeUp

nowPlayingInfo :: (Event -> Aff Unit) -> QueuedTrack -> Html Unit
nowPlayingInfo postEvent (QueuedTrack track) = Html.div $ do
  let
    onClickGoToAlbum = Html.onClick $ launchAff_ $
      postEvent $ Event.NavigateTo
        (Navigation.Album track.albumId)
        Event.RecordHistory

  -- This structure roughly follows that of the album view.
  Html.addClass "album-info"
  Html.div $ do
    Html.addClass "cover"
    let alt = track.title <> " by " <> track.artist
    Html.img (Model.thumbUrl track.albumId) alt $ Html.addClass "backdrop"
    Html.img (Model.thumbUrl track.albumId) alt $ Html.addClass "lowres"
    Html.img (Model.coverUrl track.albumId) alt $ pure unit
    onClickGoToAlbum
  Html.hgroup $ do
    Html.h1 $ Html.text track.title
    Html.h2 $ do
      Html.addClass "artist"
      Html.text track.artist
    Html.h2 $ do
      Html.addClass "album-title"
      Html.text track.album
      onClickGoToAlbum

nothingPlayingInfo :: Html Unit
nothingPlayingInfo = Html.div $ do
  Html.addClass "nothing-playing"
  Html.text "Nothing playing right now"
