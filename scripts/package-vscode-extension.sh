#!/usr/bin/env bash
set -euo pipefail

if [[ "$#" -lt 4 ]]; then
  echo "usage: $0 VERSION SOURCE_DIR OUTPUT_VSIX SYSTEM=BINARY [...]" >&2
  exit 2
fi

version="$1"
source_dir="$2"
output="$3"
shift 3

mkdir -p "$(dirname "$output")"
output="$(cd "$(dirname "$output")" && pwd)/$(basename "$output")"

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

vsix_root="$work_dir/vsix"
extension_root="$vsix_root/extension"
mkdir -p "$extension_root/bin"
cp -R "$source_dir/." "$extension_root/"

# Keep the extension manifest version in sync with the Cargo release version.
sed -i.bak -E "s/\"version\": \"[^\"]+\"/\"version\": \"$version\"/" \
  "$extension_root/package.json"
rm -f "$extension_root/package.json.bak"

for binary in "$@"; do
  system="${binary%%=*}"
  path="${binary#*=}"
  cp "$path" "$extension_root/bin/any-lsp-$system"
  chmod +x "$extension_root/bin/any-lsp-$system"
done

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
    <Identity Language="en-US" Id="beleap.any-lsp" Version="$version" Publisher="beleap" />
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

(cd "$vsix_root" && zip -qr "$output" .)
