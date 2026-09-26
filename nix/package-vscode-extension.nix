{ pkgs, version, binaries }:

let
  copyBinaries = pkgs.lib.concatStringsSep "\n" (
    pkgs.lib.mapAttrsToList (system: binary:
      let
        binaryPath = pkgs.lib.escapeShellArg (toString binary);
      in
      ''
        if [[ "$(head -c 2 ${binaryPath})" == '#!' ]]; then
          echo "expected a native executable, got a script wrapper: ${binaryPath}" >&2
          exit 1
        fi
        cp ${binaryPath} "$extension_root/bin/any-lsp-${system}"
        chmod +x "$extension_root/bin/any-lsp-${system}"
      '')
      binaries
  );
in
pkgs.stdenvNoCC.mkDerivation {
  pname = "any-lsp-vscode";
  inherit version;
  src = ../vscode-extension;

  nativeBuildInputs = [ pkgs.zip ];
  dontConfigure = true;
  dontBuild = true;

  installPhase = ''
    set -euo pipefail

    work_dir="$(mktemp -d)"
    trap 'rm -rf "$work_dir"' EXIT

    vsix_root="$work_dir/vsix"
    extension_root="$vsix_root/extension"
    mkdir -p "$extension_root/bin"
    cp -R "$src/." "$extension_root/"

    # Keep the extension manifest version in sync with the Cargo release version.
    sed -i.bak -E "s/\"version\": \"[^\"]+\"/\"version\": \"${version}\"/" \
      "$extension_root/package.json"
    rm -f "$extension_root/package.json.bak"

    # VS Code rewrites package.json with installation metadata after extraction.
    chmod u+w "$extension_root/package.json"

    ${copyBinaries}

    cat > "$vsix_root/[Content_Types].xml" <<'EOF'
    <?xml version="1.0" encoding="utf-8"?>
    <Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
      <Default Extension="json" ContentType="application/json" />
      <Default Extension="js" ContentType="application/javascript" />
      <Default Extension="md" ContentType="text/markdown" />
      <Default Extension="vsixmanifest" ContentType="text/xml" />
      <Override PartName="/extension.vsixmanifest" ContentType="text/xml" />
    </Types>
    EOF

    cat > "$vsix_root/extension.vsixmanifest" <<EOF
    <?xml version="1.0" encoding="utf-8"?>
    <PackageManifest Version="2.0.0" xmlns="http://schemas.microsoft.com/developer/vsx-schema/2011">
      <Metadata>
        <Identity Language="en-US" Id="any-lsp" Version="$version" Publisher="beleap" />
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

    mkdir -p "$out"
    (cd "$vsix_root" && zip -qr "$out/any-lsp-vscode-${version}.vsix" .)
  '';
}
