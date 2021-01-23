-- Musium -- Music playback daemon with web-based library browser
-- Copyright 2019 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module Main where

import Data.Tuple (Tuple (Tuple))
import Data.Maybe (Maybe (Nothing, Just))
import Effect (Effect)
import Effect.Aff (Aff, forkAff, launchAff_, joinFiber)
import Effect.Aff.Bus as Bus
import Effect.Class (liftEffect)
import Effect.Class.Console as Console
import Prelude

import Dom as Dom
import Event (HistoryMode (NoRecordHistory))
import Event as Event
import History as History
import Model as Model
import Navigation as Navigation
import State (AppState)
import State as State

main :: Effect Unit
main = launchAff_ $ do
  -- Set up a message bus where we can deliver events to the main loop, and a
  -- minimal initial UI, to ensure that we load something quickly.
  Tuple busOut busIn <- Bus.split <$> Bus.make

  -- Begin loading the albums asynchronously.
  fiberAlbums <- forkAff $ do
    albums <- Model.getAlbums
    pure albums

  -- Now we are ready to start building the UI. Remove the spinner to make room.
  liftEffect $ do
    loader <- Dom.getElementById "loader"
    case loader of
      Just elem -> Dom.removeChild elem Dom.body
      Nothing -> Console.log "Error, the loader should be present."

  initialState <- liftEffect $ State.new busIn

  liftEffect $ History.pushState Navigation.Library "Musium"
  liftEffect $ History.onPopState $ launchAff_ <<< case _ of
    -- For navigation events triggered by the back button, we don't record that
    -- navigation, as it is already present in the history stack.
    Nothing -> initialState.postEvent $ Event.NavigateTo Navigation.Library NoRecordHistory
    Just location -> initialState.postEvent $ Event.NavigateTo location NoRecordHistory

  -- After building the initial state, wait for the albums, and then send the
  -- initialize event. We don't post the event from the load handler, because
  -- in some cases it never gets handled then. Maybe a Bus.write silently drops
  -- the value when there is nothing reading from it yet?
  albums <- joinFiber fiberAlbums
  stateInitialized <- State.handleEvent (Event.Initialize albums) initialState

  -- The main loop handles events in a loop.
  let
    pump :: AppState -> Aff Unit
    pump state = do
      event <- Bus.read busOut
      newState <- State.handleEvent event state
      pump newState

  pump stateInitialized
