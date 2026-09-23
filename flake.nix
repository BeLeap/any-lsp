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

          vscodeExtension = pkgs.stdenvNoCC.mkDerivation {
            pname = "any-lsp-vscode";
            version = cargoToml.package.version;
            src = ./vscode-extension;

            nativeBuildInputs = [ pkgs.zip ];

            dontConfigure = true;
            dontBuild = true;

            installPhase = ''
              vsixRoot="$TMPDIR/vsix"
              mkdir -p "$vsixRoot/extension/bin" "$out"
              cp -r . "$vsixRoot/extension"
              cp ${package}/bin/any-lsp "$vsixRoot/extension/bin/any-lsp"
              chmod +x "$vsixRoot/extension/bin/any-lsp"

              cat > "$vsixRoot/[Content_Types].xml" <<'EOF'
              <?xml version="1.0" encoding="utf-8"?>
              <Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
                <Default Extension="json" ContentType="application/json" />
                <Default Extension="js" ContentType="application/javascript" />
                <Default Extension="md" ContentType="text/markdown" />
                <Default Extension="any-lsp" ContentType="application/octet-stream" />
                <Default Extension="vsixmanifest" ContentType="text/xml" />
                <Override PartName="/extension.vsixmanifest" ContentType="text/xml" />
              </Types>
              EOF

              cat > "$vsixRoot/extension.vsixmanifest" <<'EOF'
              <?xml version="1.0" encoding="utf-8"?>
              <PackageManifest Version="2.0.0" xmlns="http://schemas.microsoft.com/developer/vsx-schema/2011">
                <Metadata>
                  <Identity Language="en-US" Id="beleap.any-lsp" Version="${cargoToml.package.version}" Publisher="beleap" />
                  <DisplayName>any-lsp</DisplayName>
                  <Description xml:space="preserve">Language-agnostic definition and reference navigation for every file.</Description>
                  <Categories>Programming Languages</Categories>
                  <Properties>
                    <Property Id="Microsoft.VisualStudio.Code.Engine" Value=">=1.85.0" />
                  </Properties>
                </Metadata>
                <Installation>
                  <InstallationTarget Version="[1.0,)" Id="Microsoft.VisualStudio.Code" />
                </Installation>
                <Dependencies />
                <Assets>
                  <Asset Type="Microsoft.VisualStudio.Code.Manifest" Path="extension/package.json" />
                </Assets>
              </PackageManifest>
              EOF

              (cd "$vsixRoot" && zip -qr "$out/any-lsp-vscode-${cargoToml.package.version}.vsix" .)
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
            packages = [ pkgs.cargo pkgs.cargo-edit pkgs.clippy pkgs.ripgrep pkgs.rustc pkgs.rustfmt ];
          };
        });
    };
}
