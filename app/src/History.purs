-- Musium -- Music playback daemon with web-based library browser
-- Copyright 2020 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module History
  ( pushState
  , onPopState
  ) where

import Data.Maybe (Maybe (Nothing, Just))
import Data.Function.Uncurried (Fn3, runFn3)
import Effect (Effect)
import Prelude

import Navigation (Location)

foreign import pushStateImpl :: Fn3 Location String String (Effect Unit)
foreign import onPopStateImpl :: Fn3 (Maybe Location) (Location -> Maybe Location) (Maybe Location -> Effect Unit) (Effect Unit)

pushState :: Location -> String -> String -> Effect Unit
pushState state title url = runFn3 pushStateImpl state title url

onPopState :: (Maybe Location -> Effect Unit) -> Effect Unit
onPopState handler = runFn3 onPopStateImpl Nothing Just handler
