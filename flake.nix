{
  description = "virtty: a terminal control plane for Incus";

  nixConfig = {
    extra-substituters = [ "https://bresilla.cachix.org" ];
    extra-trusted-public-keys = [
      "bresilla.cachix.org-1:jpce/F2++w5NrhkQ19Lptd21Ck7mXT+FWC1ramXLipA="
    ];
  };

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs?rev=4c1018dae018162ec878d42fec712642d214fdfa";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    { nixpkgs, flake-utils, ... }:
    let
      manifest = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package;
    in
    flake-utils.lib.eachSystem [ "x86_64-linux" "aarch64-linux" ] (
      system:
      let
        pkgs = import nixpkgs { inherit system; };
        lib = pkgs.lib;
        virtty = pkgs.rustPlatform.buildRustPackage {
          pname = manifest.name;
          inherit (manifest) version;
          src = lib.cleanSource ./.;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [ "--bin" manifest.name ];
          # The suite runs in CI; the package proves only that the binary starts.
          doCheck = false;
          doInstallCheck = true;
          installCheckPhase = ''
            runHook preInstallCheck
            "$out/bin/${manifest.name}" --version
            "$out/bin/${manifest.name}" --help > /dev/null
            runHook postInstallCheck
          '';
          meta = {
            inherit (manifest) description;
            homepage = "https://github.com/bresilla/virtty";
            license = lib.licenses.mit;
            mainProgram = manifest.name;
            platforms = lib.platforms.linux;
          };
        };
      in
      {
        packages = {
          inherit virtty;
          default = virtty;
        };

        apps.default = {
          type = "app";
          program = lib.getExe virtty;
          meta.description = "Run virtty";
        };

        checks = { inherit virtty; };

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            rustc
            cargo
            rustfmt
            clippy
            rust-analyzer
            git-cliff
            pkg-config
          ];
        };
      }
    );
}
