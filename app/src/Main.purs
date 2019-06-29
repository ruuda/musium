module Main where

import Affjax as Http
import Affjax.ResponseFormat as Http.ResponseFormat
import Control.Monad.Error.Class (class MonadThrow, throwError)
import Data.Argonaut.Decode (decodeJson, getField) as Json
import Data.Argonaut.Decode.Class (class DecodeJson)
import Data.Either (Either (..))
import Data.Foldable (for_)
import Effect (Effect)
import Effect.Aff (Aff)
import Effect.Aff as Aff
import Effect.Class.Console as Console
import Effect.Exception (Error, error)
import Prelude

fatal :: forall m a. MonadThrow Error m => String -> m a
fatal = error >>> throwError

data Album = Album
  { id :: String
  , title :: String
  , artist :: String
  , sortArtist :: String
  , date :: String
  }

instance decodeJsonAlbum :: DecodeJson Album where
  decodeJson json = do
    obj        <- Json.decodeJson json
    id         <- Json.getField obj "id"
    title      <- Json.getField obj "title"
    artist     <- Json.getField obj "artist"
    sortArtist <- Json.getField obj "sort_artist"
    date       <- Json.getField obj "date"
    pure $ Album { id, title, artist, sortArtist, date }

getAlbums :: Aff (Array Album)
getAlbums = do
  response <- Http.get Http.ResponseFormat.json "http://localhost:8233/albums"
  case response.body of
    Left err -> fatal $ "Failed to retrieve albums: " <> Http.printResponseFormatError err
    Right json -> case Json.decodeJson json of
      Left err -> fatal $ "Failed to parse albums: " <> err
      Right albums -> pure albums

logAlbums :: Aff Unit
logAlbums = do
  albums <- getAlbums
  for_ albums (\(Album a) -> Console.log $ show a)

greet :: String -> String
greet name = "Hello, " <> name <> "!"

main :: Effect Unit
main = do
  Console.log (greet "World")
  Aff.launchAff_ logAlbums
