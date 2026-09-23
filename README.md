# any-lsp

`any-lsp` is a small language-agnostic Rust language server. It uses [ripgrep](https://github.com/BurntSushi/ripgrep) to find exact text matches, then exposes them through the standard Language Server Protocol:

- `textDocument/definition` — jump to likely declarations, headings, or prose definitions.
- `textDocument/references` — list matching occurrences across the workspace.

It does not need a parser, grammar, or language-specific index. That makes it useful for source code, Markdown, configuration files, documentation, and natural-language notes. Open buffers are searched from memory, so unsaved edits are included.

## Run

Rust and `rg` are recommended for local development. The server communicates over stdin/stdout using LSP's `Content-Length` framing:

```sh
cargo run -- --root /path/to/workspace
```

The canonical package is the Nix flake:

```sh
nix run . -- --root /path/to/workspace
nix build .
```

The flake wraps the executable with `ripgrep` on `PATH`, so the runtime does
not depend on a separately installed `rg`. `nix develop` provides Rust, Cargo,
formatting tools, and ripgrep for local development.

The client can pass optional `initializationOptions`:

```json
{
  "maxResults": 1000,
  "caseSensitive": true,
  "include": ["**/*.py", "**/*.md"],
  "exclude": ["vendor/**"]
}
```

When `rg` is unavailable, a built-in text-search fallback keeps open-buffer and small-workspace use functional.

## Try it in Helix

This repository includes an opt-in Helix language example in
`.helix/languages.toml`. It does not override Helix's built-in Rust, Markdown,
TOML, or Nix definitions. The example server is launched through the flake's
default app, `nix run --quiet .#`.

Open the fixture with:

```sh
hx examples/any-lsp-example.any-lsp-example
```

Place the cursor on `any_lsp_demo` and use `gd` to jump to its declaration or
`gr` to list its references. To try the example language on another buffer,
use `:set-language any-lsp-example` inside Helix.

## Try it in VS Code

The VS Code extension registers any-lsp's definition and reference providers
for every local file. VS Code keeps existing providers such as rust-analyzer
active and combines their results with any-lsp's text-search results.

Build and install the Nix-packaged extension:

```sh
nix build .#vscode-extension
code --install-extension "$(find "$(nix path-info .#vscode-extension)" -name '*.vsix' -print -quit)"
```

The VSIX bundles the matching `any-lsp` executable. For development, set
`anyLsp.serverPath` to another executable, or leave it empty to use
`any-lsp` from `PATH` when the extension is not using the bundled package.

The extension supports these settings:

- `anyLsp.maxResults`
- `anyLsp.caseSensitive`
- `anyLsp.include`
- `anyLsp.exclude`

## Development

```sh
cargo test
cargo fmt --check
```
