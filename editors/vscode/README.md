# Nyra for VS Code

- **Errors as you type**, with their codes and a link to what they mean, from the compiler's language server
  (`nyra lsp`).
- **Hover** (types, signatures, what an error code means), **completion** (names in scope, fields and methods after
  `.`, module functions, named options of the `use web` functions), **signature help**, **go to definition**, the
  **outline**, **formatting** (`nyra fmt`) and **quick fixes** for errors that have a certain fix.
- Syntax highlighting for `.nyra` files, including expressions inside `{ }` in strings, and the Nyra icon for `.nyra`
  files in the explorer and on editor tabs. Semicolons are highlighted as errors.

The extension has no dependencies: its language client is the small `extension.js`. It needs `nyra` 0.9 or later.

## Install (from this repo)

1. Build the compiler (`cargo build --release`) and put `target/release/nyra` on your `PATH`, or set `nyra.path`.
2. Copy this folder into your VS Code extensions folder and reload VS Code:

```bash
cp -r editors/vscode ~/.vscode/extensions/spakxd.nyra-0.5.0
```

On Windows the folder is `%USERPROFILE%\.vscode\extensions\`. Remove an older `spakxd.nyra-*` folder first.

## Settings

- `nyra.path` (default `nyra`): the `nyra` executable.
- `nyra.lsp.enabled` (default `true`): start the language server.
- `nyra.formatOnSave` (default `false`): format on save. VS Code's `editor.formatOnSave` works too.

The command **Nyra: Restart the language server** restarts it (after you rebuilt `nyra`). More in
[docs/EDITORS.md](../../docs/EDITORS.md).

VS Code uses the language's icon when the file icon theme has none of its own for `.nyra`; a theme that draws
every file itself (such as Material Icon Theme) keeps its own icon.

For the icon of `.nyra` files in Windows Explorer, see `tools/windows/install-file-icon.ps1`.
