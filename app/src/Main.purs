module Main where

import Affjax as Http
import Affjax.ResponseFormat as Http.ResponseFormat
import Data.Argonaut.Core as Json
import Data.Either (Either (..))
import Effect (Effect)
import Effect.Aff (Aff)
import Effect.Aff as Aff
import Effect.Class.Console as Console
import Prelude

getAlbums :: Aff Unit
getAlbums = do
  response <- Http.get Http.ResponseFormat.json "http://localhost:8233/albums"
  case response.body of
    Left err -> Console.log $ "Failed to retrieve albums: " <> Http.printResponseFormatError err
    Right json -> Console.log $ Json.stringify json

greet :: String -> String
greet name = "Hello, " <> name <> "!"

main :: Effect Unit
main = do
  Console.log (greet "World")
  Aff.launchAff_ getAlbums
