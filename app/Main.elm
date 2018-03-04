import Html exposing (Html)
import Html.Attributes as Html
import Http
import Json.Decode as Json

apiHost : String
apiHost = "http://localhost:8233"

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

type Model
  = Loading
  | AlbumList (List Album)
  | Failed String

init : (Model, Cmd Msg)
init =
  ( Loading
  , getAlbums
  )

-- UPDATE

type Msg
  = LoadAlbums (Result Http.Error (List Album))

update : Msg -> Model -> (Model, Cmd Msg)
update msg model =
  case msg of
    LoadAlbums (Ok albums) ->
      (AlbumList (List.sortBy .date albums), Cmd.none)
    LoadAlbums (Err _) ->
      (Failed "Failed to retrieve album list.", Cmd.none)

-- VIEW

view : Model -> Html Msg
view model =
  case model of
    Loading ->
      Html.text "loading ..."
    Failed message ->
      Html.text message
    AlbumList albums ->
      Html.div [Html.id "album-list"] (List.map viewAlbum albums)

viewAlbum : Album -> Html Msg
viewAlbum album =
  Html.div [ Html.class "album" ]
    -- TODO: Serve album covers directly.
    [ Html.img [Html.src (apiHost ++ "/cover/" ++ (String.dropRight 3 album.id) ++ "101")] []
    , Html.h2 [] [Html.text album.title]
    , Html.p [] [Html.text album.artist]
    , Html.p [Html.class "date"] [Html.text (String.left 4 album.date)]
    ]

-- SUBSCRIPTIONS

subscriptions : Model -> Sub Msg
subscriptions model =
  Sub.none

-- HTTP

getAlbums : Cmd Msg
getAlbums =
  let
    url = apiHost ++ "/albums"
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
