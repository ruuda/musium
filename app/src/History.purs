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
import Navigation as Navigation

foreign import pushStateImpl :: Fn3 String String String (Effect Unit)
foreign import onPopStateImpl :: Fn3 (Maybe String) (String -> Maybe String) (Maybe String -> Effect Unit) (Effect Unit)

pushState :: Location -> String -> Effect Unit
pushState location title =
  let
    url = Navigation.toUrl location
  in
    -- We reuse the url as the state. Previously I tried storing the Location
    -- directly, but that lead to pattern match failures, presumably PureScript
    -- ADTs don't round-trip well through the history API.
    runFn3 pushStateImpl url title url

onPopState :: (Maybe Location -> Effect Unit) -> Effect Unit
onPopState handler = runFn3 onPopStateImpl Nothing Just $ \loc -> case loc of
  Just url -> handler $ Just $ Navigation.fromUrl url
  Nothing  -> handler $ Nothing
