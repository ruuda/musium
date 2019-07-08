-- Mindec -- Music metadata indexer
-- Copyright 2019 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module Dom
  ( Element
  , appendChild
  , appendText
  , body
  , createElement
  , getElementById
  ) where

import Data.Function.Uncurried (Fn2, runFn2, Fn3, runFn3)
import Effect (Effect)
import Prelude
import Data.Maybe (Maybe (..))

foreign import data Element :: Type

foreign import createElement :: String -> Effect Element
foreign import body :: Element

foreign import appendChildImpl :: Fn2 Element Element (Effect Unit)
foreign import appendTextImpl :: Fn2 Element String (Effect Unit)
foreign import getElementByIdImpl :: Fn3 String (Element -> Maybe Element) (Maybe Element) (Effect (Maybe Element))

appendChild :: Element -> Element -> Effect Unit
appendChild container child = runFn2 appendChildImpl container child

appendText :: Element -> String -> Effect Unit
appendText container text = runFn2 appendTextImpl container text

getElementById :: String -> Effect (Maybe Element)
getElementById id = runFn3 getElementByIdImpl id Just Nothing
