let
  pkgs = import (import ../nixpkgs-pinned.nix) {
    config.android_sdk.accept_license = true;
  };
in
  pkgs.buildEnv {
    name = "mindec-devenv";
    paths = [
      pkgs.androidenv.androidPkgs_9_0.androidsdk
      pkgs.gradle
      pkgs.mkdocs
      pkgs.rustup
    ];
  }
