# any-lsp for VS Code

This extension registers any-lsp's definition and reference providers for all
local files. VS Code combines those results with providers from other language
extensions such as rust-analyzer.

The Nix package bundles the matching `any-lsp` executable. During development,
set `anyLsp.serverPath` to a locally built executable or make `any-lsp`
available on `PATH`.
