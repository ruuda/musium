let
  pkgs = import (import ../nixpkgs-pinned.nix) {
    config.android_sdk.accept_license = true;
  };
  emulator = pkgs.androidenv.emulateApp {
    name = "emulate-mindec";
    platformVersion = "28";
    abiVersion = "x86_64";
    useGoogleAPIs = false;
    enableGPU = false;
    systemImageType = "default";
  };
in
  pkgs.buildEnv {
    name = "mindec-devenv";
    paths = [
      emulator
      pkgs.androidenv.androidPkgs_9_0.androidsdk
      pkgs.gradle
      pkgs.mkdocs
      pkgs.rustup
    ];
  }
