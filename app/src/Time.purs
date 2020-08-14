-- Mindec -- Music metadata indexer
-- Copyright 2020 Ruud van Asseldonk
--
-- Adapted from Sempervivum (github.com/ruuda/sempervivum), which is copyright
-- 2020 Ruud van Asseldonk, licensed Apache 2.0.
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module Time
  ( Duration
  , Instant
  , add
  , fromSeconds
  , getCurrentInstant
  , mean
  , subtract
  , toSeconds
  ) where

import Prelude

import Data.Function.Uncurried (Fn2, Fn5, runFn2, runFn5)
import Effect (Effect)

foreign import data Instant :: Type

foreign import addSecondsImpl :: Fn2 Number Instant Instant
foreign import diffSecondsImpl :: Fn2 Instant Instant Number
foreign import eqInstantImpl :: Fn2 Instant Instant Boolean
foreign import getCurrentInstant :: Effect Instant
foreign import ordInstantImpl :: Fn5 Ordering Ordering Ordering Instant Instant Ordering

instance eqInstant :: Eq Instant where
  eq = runFn2 eqInstantImpl

instance ordInstant :: Ord Instant where
  compare = runFn5 ordInstantImpl LT EQ GT

-- Duration represents a number of seconds, but the inner value should not be
-- exposed outside of this module. Durations are signed.
newtype Duration = Duration Number
derive instance eqDuration :: Eq Duration
derive instance ordDuration :: Ord Duration

-- Note that the argument is Number, not Int. The range of a signed 32-bit
-- number of milliseconds is -24.8 to +24.8 days, so an Int is unable to
-- represent long time spans.
addSeconds :: Number -> Instant -> Instant
addSeconds = runFn2 addSecondsImpl

diffSeconds :: Instant -> Instant -> Number
diffSeconds = runFn2 diffSecondsImpl

add :: Duration -> Instant -> Instant
add (Duration secs) = addSeconds secs

subtract :: Instant -> Instant -> Duration
subtract t0 t1 = fromSeconds $ diffSeconds t0 t1

fromSeconds :: Number -> Duration
fromSeconds = Duration

toSeconds :: Duration -> Number
toSeconds (Duration secs) = secs

-- Return the instant half-way between t0 and t1.
mean :: Instant -> Instant -> Instant
mean t0 t1 = addSeconds (0.5 * diffSeconds t0 t1) t0
