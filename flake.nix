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
            nativeBuildInputs = [ pkgs.makeWrapper ];
            nativeCheckInputs = [ pkgs.ripgrep ];

            doCheck = true;

            postInstall = ''
              wrapProgram $out/bin/any-lsp \
                --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.ripgrep ]}
            '';

            meta = {
              description = "A language-agnostic LSP server powered by ripgrep";
              mainProgram = "any-lsp";
              license = pkgs.lib.licenses.mit;
            };
          };
        in
        {
          default = package;
          any-lsp = package;
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
        });

      devShells = forAllSystems ({ pkgs }:
        {
          default = pkgs.mkShell {
            packages = [ pkgs.cargo pkgs.cargo-edit pkgs.clippy pkgs.ripgrep pkgs.rustc pkgs.rustfmt ];
          };
        });
    };
}
