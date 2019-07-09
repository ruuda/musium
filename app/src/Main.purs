-- Mindec -- Music metadata indexer
-- Copyright 2019 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module Main where

import Data.Array as Array
import Effect (Effect)
import Effect.Aff (launchAff_)
import Effect.Class (liftEffect)
import Effect.Class.Console as Console
import Prelude

import View as View
import Dom as Dom
import Html as Html
import Model as Model

main :: Effect Unit
main = launchAff_ $ do
  albums <- Model.getAlbums
  Console.log ("Loaded albums: " <> (show $ Array.length albums))
  liftEffect $ Html.appendTo Dom.body $ View.renderAlbumList albums
