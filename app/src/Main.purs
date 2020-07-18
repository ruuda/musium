-- Mindec -- Music metadata indexer
-- Copyright 2019 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module Main where

import Data.Tuple (Tuple (Tuple))
import Data.Maybe (Maybe (Just, Nothing))
import Effect (Effect)
import Effect.Aff (Aff, forkAff, launchAff_)
import Effect.Aff.Bus as Bus
import Effect.Class (liftEffect)
import Effect.Class.Console as Console
import Prelude

import Dom as Dom
import History as History
import Model as Model
import State as State

main :: Effect Unit
main = launchAff_ $ do
  -- Set up a message bus where we can deliver events to the main loop, and a
  -- minimal initial UI, to ensure that we load something quickly.
  Tuple busOut busIn <- Bus.split <$> Bus.make
  initialState <- liftEffect $ State.new busIn

  -- Begin loading the albums asynchronously. When done, post an event to the
  -- main loop to display these albums.
  _fiber <- forkAff $ do
    albums <- Model.getAlbums
    Console.log "Loaded albums"
    initialState.appState.postEvent $ State.EventInitialize albums

  -- TODO: Properly integrate history.
  liftEffect $ History.onPopState $ \_state -> do
    albumView <- Dom.getElementById "album-view"
    case albumView of
      Just av -> Dom.removeChild av Dom.body
      Nothing -> pure unit

  -- The main loop handles events in a loop.
  let
    pump :: State.State -> Aff Unit
    pump state = do
      event        <- Bus.read busOut
      newAppState  <- State.handleEvent event state.appState
      newViewState <- liftEffect $ State.updateView newAppState state.viewState
      pump { appState: newAppState, viewState: newViewState }

  pump initialState
