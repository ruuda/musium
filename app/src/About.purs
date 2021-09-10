-- Musium -- Music playback daemon with web-based library browser
-- Copyright 2021 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module About
  ( new
  ) where

import Effect.Aff (Aff, launchAff_)
import Prelude

import Html (Html)
import Html as Html
import Model as Model
import Event (Event)
import Event as Event

new :: (Event -> Aff Unit) -> Html Unit
new postEvent = Html.div $ do
  Html.setId "about-inner"

  Html.div $ do
    Html.setId "about-library"
    Html.addClass "about-section"
    Html.h1 $ Html.text "Library"
    Html.p $ do
      Html.span $ do
        -- TODO: Add stats endpoint, load actual values here.
        Html.addClass "value"
        Html.text "1000"
      Html.span $ Html.text "tracks"
    Html.p $ do
      Html.span $ do
        Html.addClass "value"
        Html.text "100"
      Html.span $ Html.text "albums"
    Html.p $ do
      Html.span $ do
        Html.addClass "value"
        Html.text "10"
      Html.span $ Html.text "artists"

  Html.div $ do
    Html.setId "about-scan"
    Html.addClass "about-section"
    Html.button $ do
      Html.addClass "scan-library"
      Html.text "Rescan library"
      Html.onClick $ launchAff_ $ do
        status <- Model.startScan
        postEvent $ Event.UpdateScanStatus status
