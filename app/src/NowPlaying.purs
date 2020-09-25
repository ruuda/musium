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
import Prelude

import Dom (Element)
import Html (Html)
import Html as Html
import Model (Album (Album), Track (Track))
import Model as Model

volumeControls :: Html Unit
volumeControls = Html.div $ do
  Html.addClass "volume-controls"
  _volumeBar <- Html.div $ do
    Html.addClass "indicator"
    Html.div $ ask

  Html.button $ do
    Html.addClass "volume-down"
    Html.text "V-"
  Html.button $ do
    Html.addClass "volume-up"
    Html.text "V+"

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
