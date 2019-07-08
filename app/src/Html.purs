-- Mindec -- Music metadata indexer
-- Copyright 2019 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module Html
  ( Html
  , addClass
  , appendTo
  , div
  , img
  , li
  , node
  , onClick
  , setId
  , span
  , text
  , ul
  ) where

import Control.Monad.Reader.Trans (ReaderT (..))
import Dom (Element)
import Dom as Dom
import Effect (Effect)
import Prelude

-- An effect that builds nodes and appends them to the parent.
type Html a = ReaderT Element Effect a

appendTo :: Html Unit -> Element -> Effect Unit
appendTo (ReaderT f) container = f container

node :: String -> Html Unit -> Html Unit
node tagName (ReaderT children) =
  ReaderT $ \container -> do
    self <- Dom.createElement tagName
    children self
    Dom.appendChild self container

text :: String -> Html Unit
text value = ReaderT $ \container -> Dom.appendText value container

setId :: String -> Html Unit
setId id = ReaderT $ \container -> Dom.setId id container

addClass :: String -> Html Unit
addClass className = ReaderT $ \container -> Dom.addClass className container

onClick :: Html Unit -> Html Unit
onClick (ReaderT callback) = ReaderT $ \container ->
  Dom.addEventListener "click" (callback container) container

div :: Html Unit -> Html Unit
div children = node "div" children

span :: Html Unit -> Html Unit
span children = node "span" children

ul :: Html Unit -> Html Unit
ul children = node "ul" children

li :: Html Unit -> Html Unit
li children = node "li" children

img :: String -> String -> Html Unit
img src alt = ReaderT $ \container -> do
  self <- Dom.createElement "img"
  Dom.setAttribute "src" src self
  Dom.setAttribute "alt" alt self
  Dom.appendChild self container
