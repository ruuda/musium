-- Musium -- Music playback daemon with web-based library browser
-- Copyright 2020 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module LocalStorage
  ( set
  , get
  ) where

import Data.Function.Uncurried (Fn2, runFn2, Fn3, runFn3)
import Data.Maybe (Maybe (Nothing, Just))
import Effect (Effect)
import Prelude

foreign import getImpl :: forall a. Fn3 (Maybe a) (a -> Maybe a) String (Effect (Maybe a))
foreign import setImpl :: forall a. Fn2 String a (Effect Unit)

set :: forall a. String -> a -> Effect Unit
set = runFn2 setImpl

get :: forall a. String -> Effect (Maybe a)
get = runFn3 getImpl Nothing Just

