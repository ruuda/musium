-- Mindec -- Music metadata indexer
-- Copyright 2019 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module Html
  ( Html
  , node
  , text
  , div
  , span
  , li
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

node :: String -> String -> Html Unit -> Html Unit
node tagName className (ReaderT children) =
  ReaderT $ \container -> do
    self <- Dom.createElement tagName
    Dom.setClassName className self
    children self
    Dom.appendChild self container

text :: String -> Html Unit
text value = ReaderT $ \container -> Dom.appendText value container

div :: String -> Html Unit -> Html Unit
div className children = node "div" className children

span :: String -> Html Unit -> Html Unit
span className children = node "span" className children

li :: String -> Html Unit -> Html Unit
li className children = node "li" className children
