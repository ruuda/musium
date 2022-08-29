{
  description = "Musium";

  inputs.nixpkgs.url = "nixpkgs/nixos-unstable";
  inputs.querybinder.url = "github:ruuda/querybinder";

  outputs = { self, nixpkgs, querybinder }: 
    let
      name = "musium";
      version = builtins.substring 0 8 self.lastModifiedDate;
      supportedSystems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      forAllNixpkgs = f: forAllSystems (system: f (import nixpkgs { inherit system; }));
    in
      {
        devShells = forAllNixpkgs (pkgs: {
          default = pkgs.mkShell {
            nativeBuildInputs = [
              pkgs.mkdocs
              pkgs.psc-package
              pkgs.purescript
              pkgs.rustup
              pkgs.sqlite
              # TODO: Don't hard-code the system name ...
              querybinder.packages.x86_64-linux.default
            ];
          };
        });

        packages = forAllNixpkgs (pkgs: {
          default = pkgs.rustPlatform.buildRustPackage {
            inherit name version;
            src = ./.;
            cargoLock = {
              lockFile = ./Cargo.lock;
              outputHashes = {
                "claxon-0.4.3" = "sha256-aYFNOVGl2Iiw8/u1NrL3ZprTt48OFpG9LKs1EwEAfms=";
              };
            };
            nativeBuildInputs = [ pkgs.pkg-config ];
            buildInputs = [ pkgs.sqlite pkgs.alsa-lib ];
          };
        });
      };
}
