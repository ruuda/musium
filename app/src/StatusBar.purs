-- Mindec -- Music metadata indexer
-- Copyright 2020 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module StatusBar
  ( CurrentTrack
  , StatusBarState
  , new
  , updateProgressBar
  , updateStatusBar
  ) where

import Control.Monad.Reader.Class (ask)
import Data.Int as Int
import Data.Maybe (Maybe (Nothing, Just))
import Data.Time.Duration (Milliseconds (..))
import Effect (Effect)
import Effect.Class (liftEffect)
import Effect.Aff (launchAff_)
import Effect.Aff as Aff
import Prelude

import Dom (Element)
import Dom as Dom
import Html (Html)
import Html as Html
import Model (QueuedTrack (..), TrackId)
import Model as Model
import Time (Duration)
import Time as Time

type CurrentTrack =
  { track :: TrackId
  , container :: Element
  , progressBar :: Element
  }

type StatusBarState =
  { current :: Maybe CurrentTrack
  , statusBar :: Element
  }

newCurrentTrack :: QueuedTrack -> Html CurrentTrack
newCurrentTrack (QueuedTrack currentTrack) = Html.div $ do
  Html.addClass "current-track"
  Html.addClass "fade-in"

  progressBar <- Html.div $ do
    Html.addClass "progress"
    Html.setTransform "translateX(-100%)"
    ask

  Html.div $ do
    Html.addClass "track-info"
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

  container <- ask
  pure { track: currentTrack.id, container, progressBar }

new :: Html StatusBarState
new = Html.div $ do
  Html.setId "statusbar"
  Html.addClass "empty"
  statusBar <- ask
  pure { current: Nothing, statusBar }

-- Starts the animation to remove the current "current track", and remove it
-- once the animation is done.
removeCurrentTrack :: StatusBarState -> Effect StatusBarState
removeCurrentTrack state = case state.current of
  Nothing -> pure state
  Just current -> do
    Html.withElement state.statusBar $ Html.addClass "empty"

    -- Apply the "fade-out" class to trigger the css transition that hides the
    -- node. After the animation is done, we remove the node in the Aff below.
    Html.withElement current.container $ Html.addClass "fade-out"
    launchAff_ $ do
      -- The css transition is 0.15s, so choose 0.2s to be sure the transition
      -- has ended.
      Aff.delay $ Milliseconds 200.0
      liftEffect $ Dom.removeChild current.container state.statusBar

    pure $ state { current = Nothing }

-- Add a new current track. This loses the reference to a previous one if there
-- was any, but it does not remove it from the DOM.
addCurrentTrack :: QueuedTrack -> StatusBarState -> Effect StatusBarState
addCurrentTrack track state = do
  currentTrack <- Html.withElement state.statusBar $ do
    Html.removeClass "empty"
    newCurrentTrack track

  -- The new node gets created with "fade-in" class applied. We remove it
  -- immediately to trigger the css transition to the normal state. We do need
  -- a delay for this, if we remove the class synchronously, the transition does
  -- not trigger.
  launchAff_ $ do
    Aff.delay $ Milliseconds 17.0
    liftEffect $ Html.withElement currentTrack.container $ Html.removeClass "fade-in"

  pure $ state { current = Just currentTrack }

updateStatusBar :: Maybe QueuedTrack -> StatusBarState -> Effect StatusBarState
updateStatusBar currentTrack state =
  case currentTrack of
    Just (QueuedTrack newTrack) -> case state.current of
      -- The track did not change, nothing to do.
      -- TODO: Checking this on track id is not correct, because you can queue
      -- the same track twice in a row. Instead, the client should assign a
      -- unique identifier with every enqueue operation.
      Just old | old.track == newTrack.id -> pure state
      Just old -> addCurrentTrack (QueuedTrack newTrack) =<< removeCurrentTrack state
      Nothing  -> addCurrentTrack (QueuedTrack newTrack) state

    Nothing -> case state.current of
      Just old -> removeCurrentTrack state
      Nothing  -> pure state

-- Update the progress of the current track, return delay until the next update
-- is needed. This does not confirm that the current track in the view matches
-- the track passed to this function.
updateProgressBar :: QueuedTrack -> StatusBarState -> Effect Duration
updateProgressBar (QueuedTrack currentTrack) state = do
  now <- Time.getCurrentInstant
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

  case state.current of
    Nothing -> pure unit
    Just t -> Html.withElement t.progressBar $ do
      Html.setTransition transition
      Html.setTransform transform

  -- Schedule the next update slightly before the animation completes, so we
  -- will not be too late to start the next one, which could cause a stutter.
  pure $ Time.fromSeconds $ max 0.2 $ animationDurationSeconds - 0.2
