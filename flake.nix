{
  description = "Musium";

  inputs.nixpkgs.url = "nixpkgs/nixos-unstable";
  inputs.squiller.url = "github:ruuda/squiller?ref=v0.3.0";
  inputs.squiller.inputs.nixpkgs.follows = "nixpkgs";

  outputs = { self, nixpkgs, squiller }:
    let
      supportedSystems = ["x86_64-linux" "aarch64-linux"];
      # Ridiculous boilerplate required to make flakes somewhat usable.
      forEachSystem = f:
        builtins.zipAttrsWith
          (name: values: builtins.foldl' (x: y: x // y) {} values)
          (map
            (k: builtins.mapAttrs (name: value: { "${k}" = value; }) (f k))
            supportedSystems
          );
    in
      forEachSystem (system:
        let
          name = "musium";
          version = builtins.substring 0 8 self.lastModifiedDate;
          pkgs = import nixpkgs { inherit system; };
        in
          rec {
            devShells.default = pkgs.mkShell {
              inherit name;
              nativeBuildInputs = [
                pkgs.mkdocs
                pkgs.psc-package
                pkgs.purescript
                pkgs.rustup
                pkgs.sqlite
                squiller.packages.${system}.default
              ]
              ++ packages.default.nativeBuildInputs
              ++ packages.default.buildInputs;
            };

            packages.default = pkgs.rustPlatform.buildRustPackage {
              inherit name version;
              src = ./.;
              cargoLock = {
                lockFile = ./Cargo.lock;
                outputHashes = {
                  "claxon-0.4.3" = "sha256-aYFNOVGl2Iiw8/u1NrL3ZprTt48OFpG9LKs1EwEAfms=";
                };
              };
              nativeBuildInputs = [ pkgs.pkg-config ];
              buildInputs = [
                pkgs.alsa-lib
                pkgs.sqlite
                pkgs.systemd
              ];
            };
          }
      );
}
