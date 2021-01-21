-- Musium -- Music playback daemon with web-based library browser
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
  , focusElement
  , getElementById
  , getOffsetHeight
  , getScrollTop
  , getValue
  , getWindowHeight
  , onScroll
  , onResizeWindow
  , removeChild
  , removeClass
  , renderHtml
  , scrollIntoView
  , setAttribute
  , setHeight
  , setId
  , setImage
  , setScrollTop
  , setTransform
  , setTransition
  , setValue
  , setWidth
  ) where

import Data.Function.Uncurried (Fn2, runFn2, Fn3, runFn3)
import Effect (Effect)
import Prelude
import Data.Maybe (Maybe (..))

foreign import data Element :: Type

instance eqElement :: Eq Element where
  eq = eqElementImpl

foreign import assumeElementById :: String -> Effect Element
foreign import body :: Element
foreign import clearElement :: Element -> Effect Unit
foreign import createElement :: String -> Effect Element
foreign import eqElementImpl :: Element -> Element -> Boolean
foreign import focusElement :: Element -> Effect Unit
foreign import getOffsetHeight :: Element -> Effect Number
foreign import getScrollTop :: Element -> Effect Number
foreign import getValue :: Element -> Effect String
foreign import getWindowHeight :: Effect Number
foreign import onResizeWindow :: (Effect Unit) -> (Effect Unit)
foreign import scrollIntoView :: Element -> Effect Unit
-- This actually returns a DocumentFragment at runtime, but it can be used in
-- the same way that an Element can.
foreign import renderHtml :: String -> Effect Element

foreign import addClassImpl :: Fn2 String Element (Effect Unit)
foreign import addEventListenerImpl :: Fn3 String (Effect Unit) Element (Effect Unit)
foreign import appendChildImpl :: Fn2 Element Element (Effect Unit)
foreign import appendTextImpl :: Fn2 String Element (Effect Unit)
foreign import getElementByIdImpl :: Fn3 String (Element -> Maybe Element) (Maybe Element) (Effect (Maybe Element))
foreign import onScrollImpl :: Fn2 (Effect Unit) Element (Effect Unit)
foreign import removeChildImpl :: Fn2 Element Element (Effect Unit)
foreign import removeClassImpl :: Fn2 String Element (Effect Unit)
foreign import setAttributeImpl :: Fn3 String String Element (Effect Unit)
foreign import setHeightImpl :: Fn2 String Element (Effect Unit)
foreign import setIdImpl :: Fn2 String Element (Effect Unit)
foreign import setImageImpl :: Fn3 String String Element (Effect Unit)
foreign import setScrollTopImpl :: Fn2 Number Element (Effect Unit)
foreign import setTransformImpl :: Fn2 String Element (Effect Unit)
foreign import setTransitionImpl :: Fn2 String Element (Effect Unit)
foreign import setWidthImpl :: Fn2 String Element (Effect Unit)
foreign import setValueImpl :: Fn2 String Element (Effect Unit)

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

setValue :: String -> Element -> Effect Unit
setValue v element = runFn2 setValueImpl v element

setWidth :: String -> Element -> Effect Unit
setWidth w element = runFn2 setWidthImpl w element

setHeight :: String -> Element -> Effect Unit
setHeight h element = runFn2 setHeightImpl h element

setId :: String -> Element -> Effect Unit
setId id element = runFn2 setIdImpl id element

setTransform :: String -> Element -> Effect Unit
setTransform transform element = runFn2 setTransformImpl transform element

setTransition :: String -> Element -> Effect Unit
setTransition transition element = runFn2 setTransitionImpl transition element

setScrollTop :: Number -> Element -> Effect Unit
setScrollTop off element = runFn2 setScrollTopImpl off element

setAttribute :: String -> String -> Element -> Effect Unit
setAttribute attribute value element = runFn3 setAttributeImpl attribute value element

setImage :: String -> String -> Element -> Effect Unit
setImage src alt element = runFn3 setImageImpl src alt element

addEventListener :: String -> Effect Unit -> Element -> Effect Unit
addEventListener eventName callback element = runFn3 addEventListenerImpl eventName callback element

onScroll :: Effect Unit -> Element -> Effect Unit
onScroll callback element = runFn2 onScrollImpl callback element
