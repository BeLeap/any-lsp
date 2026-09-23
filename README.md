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

## Development

```sh
cargo test
cargo fmt --check
```
