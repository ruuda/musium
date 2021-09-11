-- Musium -- Music playback daemon with web-based library browser
-- Copyright 2021 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module About
  ( AboutElements (..)
  , new
  , updateScanStatus
  ) where

import Control.Monad.Reader.Class (ask)
import Effect (Effect)
import Effect.Aff (Aff, launchAff_)
import Prelude

import Dom (Element)
import Event (Event)
import Event as Event
import Html (Html)
import Html as Html
import Model (ScanStatus (..), ScanStage (..))
import Model as Model

type AboutElements =
  { scanStatus :: Element
  }

valuePair :: String -> String -> Html Unit
valuePair lhs rhs = Html.p $ do
  Html.span $ do
    Html.addClass "value"
    Html.text lhs
  Html.span $ do
    Html.addClass "description"
    Html.text rhs

new :: (Event -> Aff Unit) -> Html AboutElements
new postEvent = Html.div $ do
  Html.setId "about-inner"

  Html.div $ do
    Html.setId "about-library"
    Html.addClass "about-section"
    Html.h1 $ Html.text "Library"
    -- TODO: Add stats endpoint, load actual values here.
    valuePair "1000" "tracks"
    valuePair "100" "albums"
    valuePair "10" "artists"

  Html.div $ do
    Html.setId "about-scan"
    Html.addClass "about-section"

    Html.button $ do
      Html.addClass "scan-library"
      Html.text "Rescan library"
      Html.onClick $ launchAff_ $ do
        status <- Model.startScan
        postEvent $ Event.UpdateScanStatus status

    Html.div $ do
      Html.setId "scan-status"
      self <- ask
      pure { scanStatus: self }

updateScanStatus :: AboutElements -> ScanStatus -> Effect Unit
updateScanStatus elems (ScanStatus status) =
  Html.withElement elems.scanStatus $ do
    Html.clear

    Html.p $ Html.span $ do
      Html.addClass "description"
      Html.text $ case status.stage of
        ScanDiscovering             -> "Discovering files …"
        ScanPreProcessingMetadata   -> "Determining which need to be processed …"
        ScanExtractingMetadata      -> "Extracting metadata …"
        ScanPreProcessingThumbnails -> "Indexing metadata …"
        ScanGeneratingThumbnails    -> "Generating thumbnails …"
        ScanDone                    -> "Scan complete"

    valuePair (show status.filesDiscovered) "files discovered"
    valuePair
      ((show status.filesProcessedMetadata) <> " of " <> (show status.filesToProcessMetadata))
      "new files processed"
    valuePair
      ((show status.filesProcessedThumbnails) <> " of " <> (show status.filesToProcessThumbnails))
      "new thumbnails extracted"
