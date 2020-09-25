-- Musium -- Music playback daemon with web-based library browser
-- Copyright 2020 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module NowPlaying
  ( nowPlayingAlbum
  , volumeControls
  ) where

import Control.Monad.Reader.Class (ask)
import Effect.Aff (launchAff_)
import Effect.Class (liftEffect)
import Prelude

import Dom (Element)
import Dom as Dom
import Html (Html)
import Html as Html
import Model (Album (Album), Decibel (Decibel), Track (Track), Volume (Volume))
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
      let percentage = max 0.0 $ min 100.0 $ (v + 20.0) / 0.3
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

nowPlayingAlbum :: Track -> Album -> Html Element
nowPlayingAlbum (Track track) (Album album) = do
  -- This structure roughly follows that of the album view.
  Html.addClass "album-info"
  Html.div $ do
    Html.addClass "cover"
    let alt = album.title <> " by " <> album.artist
    Html.img (Model.thumbUrl album.id) alt $ Html.addClass "backdrop"
    Html.img (Model.thumbUrl album.id) alt $ Html.addClass "lowres"
    Html.img (Model.coverUrl album.id) alt $ pure unit
  Html.hgroup $ do
    Html.h1 $ Html.text track.title
    Html.h2 $ do
      Html.span $ do
        Html.addClass "artist"
        Html.text track.artist
      Html.text " ⋅ "
      Html.span $ do
        Html.addClass "date"
        Html.text album.date

  ask
