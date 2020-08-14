-- Mindec -- Music metadata indexer
-- Copyright 2020 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module StatusBar
  ( renderStatusBar
  ) where

import Html (Html)
import Html as Html
import Model (QueuedTrack (..))
import Model as Model
import Prelude

renderStatusBar :: QueuedTrack -> Html Unit
renderStatusBar (QueuedTrack currentTrack) = do
  Html.img
    (Model.thumbUrl $ currentTrack.albumId)
    (currentTrack.title <> " by " <> currentTrack.artist)
    (Html.addClass "thumb")
  Html.span $ do
    Html.addClass "title"
    Html.text $ currentTrack.title
  Html.span $ do
    Html.addClass "artist"
    Html.text $ currentTrack.artist
