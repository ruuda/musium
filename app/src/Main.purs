-- Mindec -- Music metadata indexer
-- Copyright 2019 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module Main where

import Data.Tuple (Tuple (Tuple))
import Data.Maybe (Maybe (Nothing, Just))
import Effect (Effect)
import Effect.Aff (Aff, forkAff, launchAff_)
import Effect.Aff.Bus as Bus
import Effect.Class (liftEffect)
import Effect.Class.Console as Console
import Prelude

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
  initialState <- liftEffect $ State.new busIn

  -- Begin loading the albums asynchronously. When done, post an event to the
  -- main loop to display these albums.
  _fiberAlbums <- forkAff $ do
    albums <- Model.getAlbums
    Console.log "Loaded albums"
    initialState.postEvent $ Event.Initialize albums

  liftEffect $ History.pushState Navigation.Library "Mindec" "/"
  liftEffect $ History.onPopState $ launchAff_ <<< case _ of
    -- TODO: Avoid double pushes here.
    Nothing -> initialState.postEvent Event.OpenLibrary
    Just location -> case location of
      Navigation.Library     -> initialState.postEvent Event.OpenLibrary
      Navigation.Album album -> initialState.postEvent $ Event.OpenAlbum album

  -- The main loop handles events in a loop.
  let
    pump :: AppState -> Aff Unit
    pump state = do
      event <- Bus.read busOut
      newState <- State.handleEvent event state
      pump newState

  pump initialState
