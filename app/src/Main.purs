-- Mindec -- Music metadata indexer
-- Copyright 2019 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module Main where

import Data.Maybe (Maybe (Just, Nothing))
import Effect (Effect)
import Effect.Aff (launchAff_)
import Effect.Class (liftEffect)
import Effect.Class.Console as Console
import Prelude

import Dom as Dom
import History as History
import Html as Html
import Model as Model
import View as View
import State as State

main :: Effect Unit
main = launchAff_ $ do
  albums <- Model.getAlbums
  Console.log "Loaded albums"
  app <- liftEffect $ State.new albums

  liftEffect $ History.onPopState $ \_state -> do
    -- TODO: Actually inspect state, also handle initial null state.
    albumView <- Dom.getElementById "album-view"
    case albumView of
      Just av -> Dom.removeChild av Dom.body
      Nothing -> pure unit

  liftEffect $ Html.withElement Dom.body $ View.renderAlbumList albums
