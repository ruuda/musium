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
import Effect.Aff (launchAff_)
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
  Tuple busOut busIn <- Bus.split <$> Bus.make

  albums <- Model.getAlbums
  Console.log "Loaded albums"
  app <- liftEffect $ State.new busIn albums

  liftEffect $ History.onPopState $ \_state -> do
    -- TODO: Actually inspect state, also handle initial null state.
    albumView <- Dom.getElementById "album-view"
    case albumView of
      Just av -> Dom.removeChild av Dom.body
      Nothing -> pure unit
