# any-lsp for VS Code

This extension registers any-lsp's definition and reference providers for all
local files. VS Code combines those results with providers from other language
extensions such as rust-analyzer.

The Nix package bundles platform-specific `any-lsp` executables and chooses
the matching one at runtime. During development, set `anyLsp.serverPath` to a
locally built executable or make `any-lsp` available on `PATH`.
