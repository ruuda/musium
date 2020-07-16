-- Mindec -- Music metadata indexer
-- Copyright 2019 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module Main where

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

main :: Effect Unit
main = launchAff_ $ do
  albums <- Model.getAlbums

  liftEffect $ History.onPopState $ \_state -> do
    -- TODO: Actually inspect state, also handle initial null state.
    Console.log "History popped back to ..."
    Html.withElement Dom.body $ do
      Html.clear
      View.renderAlbumList albums

  liftEffect $ Html.withElement Dom.body $ View.renderAlbumList albums
