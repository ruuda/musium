-- Mindec -- Music metadata indexer
-- Copyright 2019 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module Main where

import Prelude
import Effect (Effect)
import Halogen.Aff as HA
import Halogen.VDom.Driver (runUI)

import View as View

main :: Effect Unit
main = HA.runHalogenAff do
  body <- HA.awaitBody
  runUI View.component unit body
