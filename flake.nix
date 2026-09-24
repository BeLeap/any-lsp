{
  description = "A language-agnostic LSP server powered by ripgrep";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  inputs.staticBinaries = {
    url = "path:./static-binaries";
    flake = false;
  };

  outputs = { self, nixpkgs, staticBinaries }:
    let
      cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);

      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];

      forAllSystems = function:
        nixpkgs.lib.genAttrs systems (system:
          function {
            pkgs = import nixpkgs { inherit system; };
          });
    in
    {
      packages = forAllSystems ({ pkgs }:
        let
          common = {
            pname = "any-lsp";
            version = cargoToml.package.version;
            src = ./.;

            cargoLock = { lockFile = ./Cargo.lock; };
            doCheck = true;

            meta = {
              description = "A language-agnostic LSP server powered by ripgrep";
              mainProgram = "any-lsp";
              license = pkgs.lib.licenses.mit;
            };
          };

          package = pkgs.rustPlatform.buildRustPackage common;

          staticBinary =
            if pkgs.stdenv.hostPlatform.isLinux then
              pkgs.pkgsStatic.rustPlatform.buildRustPackage common
            else
              package;

          staticBinaryPath = system: "${staticBinaries}/any-lsp-static-${system}";
          hasStaticBinary = system: builtins.pathExists (staticBinaryPath system);
          hasAnyStaticBinary = builtins.any hasStaticBinary systems;
          hasAllStaticBinaries = builtins.all hasStaticBinary systems;

          extensionBinaries =
            if hasAllStaticBinaries then
              builtins.listToAttrs (map (system: {
                name = system;
                value = staticBinaryPath system;
              }) systems)
            else if hasAnyStaticBinary then
              throw "staticBinaries input must include binaries for all supported systems"
            else {
              "${pkgs.stdenv.hostPlatform.system}" = "${staticBinary}/bin/any-lsp";
            };

          vscodeExtension = import ./nix/package-vscode-extension.nix {
            inherit pkgs;
            version = cargoToml.package.version;
            binaries = extensionBinaries;
          };
        in
        {
          default = package;
          any-lsp = package;
          static-binary = staticBinary;
          vscode-extension = vscodeExtension;
        });

      apps = forAllSystems ({ pkgs }:
        {
          default = {
            type = "app";
            program = "${self.packages.${pkgs.stdenv.hostPlatform.system}.default}/bin/any-lsp";
            meta.description = "Run any-lsp";
          };
        });

      checks = forAllSystems ({ pkgs }:
        {
          default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
          vscode-extension = self.packages.${pkgs.stdenv.hostPlatform.system}.vscode-extension;
        });

      devShells = forAllSystems ({ pkgs }:
        {
          default = pkgs.mkShell {
            packages = [ pkgs.cargo pkgs.cargo-edit pkgs.clippy pkgs.rustc pkgs.rustfmt pkgs.zip ];
          };
        });
    };
}
