import Html exposing (Html)
import Http
import Json.Decode as Json

main =
  Html.program
    { init = init
    , view = view
    , update = update
    , subscriptions = subscriptions
    }

-- DATA

type alias Album =
  { id : String
  , title : String
  , artist : String
  , sortArtist : String
  , date : String
  }

-- MODEL

type alias Model =
  { albums : List Album
  }

init : (Model, Cmd Msg)
init =
  ( Model []
  , getAlbums
  )

-- UPDATE

type Msg
  = LoadAlbums (Result Http.Error (List Album))

update : Msg -> Model -> (Model, Cmd Msg)
update msg model =
  case msg of
    LoadAlbums (Ok albums) ->
      (Model albums, Cmd.none)
    LoadAlbums (Err _) ->
      (Model [], Cmd.none)

-- VIEW

view : Model -> Html Msg
view model =
  Html.div [] (List.map viewAlbum model.albums)

viewAlbum : Album -> Html Msg
viewAlbum album =
  Html.div []
    [ Html.span [] [Html.text album.title]
    , Html.span [] [Html.text album.artist]
    ]

-- SUBSCRIPTIONS

subscriptions : Model -> Sub Msg
subscriptions model =
  Sub.none

-- HTTP

getAlbums : Cmd Msg
getAlbums =
  let
    url = "http://localhost:8233/albums"
  in
    Http.send LoadAlbums (Http.get url (Json.list decodeAlbum))

decodeAlbum : Json.Decoder Album
decodeAlbum =
  Json.map5 Album
    (Json.field "id" Json.string)
    (Json.field "title" Json.string)
    (Json.field "artist" Json.string)
    (Json.field "sort_artist" Json.string)
    (Json.field "date" Json.string)
