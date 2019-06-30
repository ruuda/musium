module AlbumComponent (component) where

import Data.Maybe (Maybe (..))
import Effect.Aff.Class (class MonadAff)
import Data.Newtype (unwrap)
import Halogen as H
import Halogen.HTML as HH
import Halogen.HTML.Events as HE
import Halogen.HTML.Properties as HP
import Prelude

import Model (Album)
import Model as Model

type State =
  { album  :: Album
  , isOpen :: Boolean
  , tracks :: Array Int
  }

data Action = Toggle

component :: forall f o m. MonadAff m => H.Component HH.HTML f Album o m
component =
  H.mkComponent
    { initialState
    , render
    , eval: H.mkEval $ H.defaultEval
      { handleAction = handleAction
      }
    }

initialState :: Album -> State
initialState album =
  { album: album
  , isOpen: false
  , tracks: []
  }

render :: forall m. State -> H.ComponentHTML Action () m
render state =
  let
    album = unwrap state.album
  in
    HH.li
      [ HE.onClick \_ -> Just Toggle ] $
      [ HH.img
        [ HP.src (Model.thumbUrl album.id)
        , HP.alt $ album.title <> " by " <> album.artist
        ]
      , HH.strong_ [ HH.text album.title ]
      , HH.text " "
      , HH.span_ [ HH.text album.artist ]
      ] <> if not state.isOpen
        then []
        else [ HH.text "OPEN" ]

handleAction :: forall o m. MonadAff m => Action -> H.HalogenM State Action () o m Unit
handleAction = case _ of
  Toggle -> do
    H.modify_ $ \state -> state { isOpen = not state.isOpen }
