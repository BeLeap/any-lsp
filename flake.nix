{
  description = "A language-agnostic LSP server powered by ripgrep";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs = { self, nixpkgs }:
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
          package = pkgs.rustPlatform.buildRustPackage {
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

          vscodeExtension = pkgs.stdenvNoCC.mkDerivation {
            pname = "any-lsp-vscode";
            version = cargoToml.package.version;
            src = ./vscode-extension;

            nativeBuildInputs = [ pkgs.zip ];

            dontConfigure = true;
            dontBuild = true;

            installPhase = ''
              mkdir -p "$out"
              bash ${./scripts/package-vscode-extension.sh} \
                "${cargoToml.package.version}" \
                "$PWD" \
                "$out/any-lsp-vscode-${cargoToml.package.version}.vsix" \
                "${pkgs.stdenv.hostPlatform.system}=${package}/bin/any-lsp"
            '';
          };
        in
        {
          default = package;
          any-lsp = package;
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
