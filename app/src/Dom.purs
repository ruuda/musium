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
  , assumeElementById
  , body
  , createElement
  , getElementById
  , setClassName
  , setId
  ) where

import Data.Function.Uncurried (Fn2, runFn2, Fn3, runFn3)
import Effect (Effect)
import Prelude
import Data.Maybe (Maybe (..))

foreign import data Element :: Type

foreign import assumeElementById :: String -> Effect Element
foreign import body :: Element
foreign import createElement :: String -> Effect Element

foreign import appendChildImpl :: Fn2 Element Element (Effect Unit)
foreign import appendTextImpl :: Fn2 String Element (Effect Unit)
foreign import getElementByIdImpl :: Fn3 String (Element -> Maybe Element) (Maybe Element) (Effect (Maybe Element))
foreign import setClassNameImpl :: Fn2 String Element (Effect Unit)
foreign import setIdImpl :: Fn2 String Element (Effect Unit)

appendChild :: Element -> Element -> Effect Unit
appendChild child container = runFn2 appendChildImpl child container

appendText :: String -> Element -> Effect Unit
appendText text container = runFn2 appendTextImpl text container

getElementById :: String -> Effect (Maybe Element)
getElementById id = runFn3 getElementByIdImpl id Just Nothing

setClassName :: String -> Element -> Effect Unit
setClassName className element = runFn2 setClassNameImpl className element

setId :: String -> Element -> Effect Unit
setId id element = runFn2 setIdImpl id element
