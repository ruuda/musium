# This file defines a Nix environment in which all required build tools are
# available. You do not *need* to use it, you can install  build tools in any
# way you see fit. The advantage of the Nix environment is that the Nixpkgs
# revision is pinned, and thereby the versions of all tools. If you can build
# a commit today, you should be able to build it three years from now. The same
# may not be true if you use the distro-provided versions. You can start a shell
# with build tools available by running `nix run` in the root of the repository.

let
  pkgs = import (import ./nixpkgs-pinned.nix) {
    config.android_sdk.accept_license = true;
  };
  emulate = pkgs.androidenv.emulateApp {
    name = "emulate-mindec";
    platformVersion = "28";
    abiVersion = "x86_64";
    systemImageType = "default";
    useGoogleAPIs = false;
    # app = app3/mindec.apk;
    package = "nl.ruuda.mindec";
    activity = "MainActivity";
  };
in
  pkgs.buildEnv {
    name = "mindec-devenv";
    paths = [
      emulate
      pkgs.androidenv.androidPkgs_9_0.androidsdk
      pkgs.gradle
      pkgs.mkdocs
      pkgs.rustup
    ];
  }
