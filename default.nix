# This file defines a Nix environment in which all required build tools are
# available. You do not *need* to use it, you can install  build tools in any
# way you see fit. The advantage of the Nix environment is that the Nixpkgs
# revision is pinned, and thereby the versions of all tools. If you can build
# a commit today, you should be able to build it three years from now. The same
# may not be true if you use the distro-provided versions. You can start a shell
# with build tools available by running `nix run` in the root of the repository.

let
  pkgs = (import ./nixpkgs-pinned.nix) {};
  python = pkgs.python3.withPackages (ps: [
    ps.pytradfri
  ]);
in
  pkgs.buildEnv {
    name = "musium-devenv";
    paths = [
      pkgs.mkdocs
      pkgs.psc-package
      pkgs.purescript
      pkgs.rustup
      python
    ];
  }
