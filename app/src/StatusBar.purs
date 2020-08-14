-- Mindec -- Music metadata indexer
-- Copyright 2020 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module StatusBar
  ( StatusBarElements
  , new
  , renderCurrentTrack
  , updateProgressBar
  ) where

import Control.Monad.Reader.Class (ask)
import Data.Int as Int
import Effect.Class (liftEffect)
import Prelude

import Dom (Element)
import Html (Html)
import Html as Html
import Model (QueuedTrack (..))
import Model as Model
import Time (Duration)
import Time as Time

type StatusBarElements =
  { progressBar :: Element
  , currentTrack :: Element
  }

new :: Html StatusBarElements
new = Html.div $ do
  Html.setId "statusbar"
  progressBar <- Html.div $ do
    Html.setId "progress"
    Html.addClass "empty"
    ask
  currentTrack <- Html.div $ do
    Html.setId "current-track"
    Html.addClass "empty"
    ask
  pure { progressBar, currentTrack }

renderCurrentTrack :: QueuedTrack -> Html Unit
renderCurrentTrack (QueuedTrack currentTrack) = do
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

-- Update the progress bar. Return delay until the next update is needed.
updateProgressBar :: QueuedTrack -> Html Duration
updateProgressBar (QueuedTrack currentTrack) = do
  now <- liftEffect $ Time.getCurrentInstant
  let
    -- Compute the completion 5 seconds from now, or at the end of the track,
    -- whichever comes first, and set that as the target. Then set a css
    -- transition, to make sure that we reach the target at the desired time.
    -- This way we get contiuous smooth updates without having to run code every
    -- frame.
    durationSeconds = Int.toNumber currentTrack.durationSeconds
    endTime = Time.add (Time.fromSeconds durationSeconds) currentTrack.startedAt
    target = min endTime $ Time.add (Time.fromSeconds 5.0) now
    position = Time.subtract target currentTrack.startedAt
    completion = max 0.0 $ min 1.0 $ (Time.toSeconds position) / durationSeconds
    transform = "translateX(" <> show (-100.0 * (1.0 - completion)) <> "%)"
    animationDuration = Time.subtract target now
    animationDurationSeconds = Time.toSeconds animationDuration
    transition = "opacity 0.5s ease-in-out, transform " <> show animationDurationSeconds <> "s linear"

  Html.setTransition transition
  Html.setTransform transform

  -- Schedule the next update slightly before the animation completes, so we
  -- will not be too late to start the next one, which could cause a stutter.
  pure $ Time.fromSeconds $ max 0.2 $ animationDurationSeconds - 0.2
