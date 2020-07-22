-- Mindec -- Music metadata indexer
-- Copyright 2019 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module Dom
  ( Element
  , addClass
  , addEventListener
  , appendChild
  , appendText
  , assumeElementById
  , body
  , clearElement
  , createElement
  , getElementById
  , getValue
  , removeChild
  , removeClass
  , setAttribute
  , setId
  , setTransform
  , window
  , onScroll
  ) where

import Data.Function.Uncurried (Fn2, runFn2, Fn3, runFn3)
import Effect (Effect)
import Prelude
import Data.Maybe (Maybe (..))

foreign import data Element :: Type

foreign import assumeElementById :: String -> Effect Element
foreign import body :: Element
foreign import clearElement :: Element -> Effect Unit
foreign import createElement :: String -> Effect Element
foreign import getValue :: Element -> Effect String
-- Not really an Element, but it is for the purpose of adding an event listener.
foreign import window :: Element

foreign import addClassImpl :: Fn2 String Element (Effect Unit)
foreign import addEventListenerImpl :: Fn3 String (Effect Unit) Element (Effect Unit)
foreign import onScrollImpl :: Fn2 (Effect Unit) Element (Effect Unit)
foreign import appendChildImpl :: Fn2 Element Element (Effect Unit)
foreign import appendTextImpl :: Fn2 String Element (Effect Unit)
foreign import getElementByIdImpl :: Fn3 String (Element -> Maybe Element) (Maybe Element) (Effect (Maybe Element))
foreign import removeChildImpl :: Fn2 Element Element (Effect Unit)
foreign import removeClassImpl :: Fn2 String Element (Effect Unit)
foreign import setAttributeImpl :: Fn3 String String Element (Effect Unit)
foreign import setIdImpl :: Fn2 String Element (Effect Unit)
foreign import setTransformImpl :: Fn2 String Element (Effect Unit)

appendChild :: Element -> Element -> Effect Unit
appendChild child container = runFn2 appendChildImpl child container

removeChild :: Element -> Element -> Effect Unit
removeChild child container = runFn2 removeChildImpl child container

appendText :: String -> Element -> Effect Unit
appendText text container = runFn2 appendTextImpl text container

getElementById :: String -> Effect (Maybe Element)
getElementById id = runFn3 getElementByIdImpl id Just Nothing

addClass :: String -> Element -> Effect Unit
addClass className element = runFn2 addClassImpl className element

removeClass :: String -> Element -> Effect Unit
removeClass className element = runFn2 removeClassImpl className element

setId :: String -> Element -> Effect Unit
setId id element = runFn2 setIdImpl id element

setTransform :: String -> Element -> Effect Unit
setTransform transform element = runFn2 setTransformImpl transform element

setAttribute :: String -> String -> Element -> Effect Unit
setAttribute attribute value element = runFn3 setAttributeImpl attribute value element

addEventListener :: String -> Effect Unit -> Element -> Effect Unit
addEventListener eventName callback element = runFn3 addEventListenerImpl eventName callback element

onScroll :: Effect Unit -> Element -> Effect Unit
onScroll callback element = runFn2 onScrollImpl callback element
