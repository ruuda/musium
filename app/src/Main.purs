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
