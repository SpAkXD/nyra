# Nyra in editors

`nyra lsp` is a language server: the compiler, answering an editor over the
[Language Server Protocol](https://microsoft.github.io/language-server-protocol/) (version 3.17, JSON-RPC on
standard input and output). Any editor with an LSP client can use it. VS Code has an extension in this repository
(`editors/vscode`); other editors need a few lines of configuration (below).

## What it does

| Feature | What you get |
| --- | --- |
| Diagnostics | The errors and warnings of `nyra check` while you type: the code (`E0201`), the message, the hint, a link to the code's entry in [ERRORS.md](ERRORS.md), and `fix:` when the error has a certain fix. Failing `ex` examples and the slow-pattern warnings (E0360-E0362) too. |
| Hover | The type of a variable (`let total: int`, a parameter, a loop variable, a script variable), the signature and comment of a function: the program's own, an imported file's (`shapes.area`), a standard module's (`math.sqrt`), a builtin, a method (`[int].push`), and with `use web` the web library's functions with all their named options. A struct, enum, field or variant. On an error or warning: what its code means (`nyra explain`). |
| Completion | Keywords, builtins, the variables in scope, the program's functions, structs and enums. After `.`: a struct's fields, the methods of the value's type (the names of the agent card), the program's functions that take that type first (`r.area()` is `area(r)`), a standard module's functions and constants (`math.`), an imported file's `pub` functions (`shapes.`), an enum's variants (`Dir.`). Inside the parentheses of a call: the parameters as named arguments (`style: `). After `use `: the standard modules and the `.nyra` files next to this one. |
| Signature help | The parameters of the function, method or struct being called, with the current one marked (also when it is given by name). |
| Go to definition | Functions, structs, enums, fields, variants, variables and parameters; a function of an imported file, and the file of a `use ./name` line. |
| Outline | The functions, structs (with their fields), enums (with their variants) and script variables. |
| Formatting | `nyra fmt`: canonical indentation and `return`; the certain fixes are applied too when the program then compiles. Only white space and the spelling of `return` change otherwise. |
| Quick fixes | Each error that has exactly one certain repair (the fixes of `nyra check --fix`) is a quick fix, and "Apply every certain fix" a `source.fixAll` action when the program then compiles. |

The server works on the editor's buffers, saved or not. A program of several files works too: `use ./name` reads
the open buffer of `name.nyra` before the file on disk, so a change in one file is seen at once in the files that
import it. A buffer is checked again 150 ms after the last change, and at once when a request needs it; on a
2,000-line file a check, a hover or a completion takes about 20 ms (release build).

## VS Code

The extension is in `editors/vscode` (version 0.5.0). It has no dependencies: its LSP client is written in its own
`extension.js`.

1. Build the compiler, `cargo build --release`, and put `target/release/nyra` (`nyra.exe` on Windows) on your
   `PATH`, or note its full path.
2. Install the extension. Copy the folder into VS Code's extensions folder and reload VS Code:

   ```bash
   cp -r editors/vscode ~/.vscode/extensions/spakxd.nyra-0.5.0
   ```

   On Windows the folder is `%USERPROFILE%\.vscode\extensions\`. Remove an older `spakxd.nyra-*` folder first.
   (With [vsce](https://github.com/microsoft/vscode-vsce), `npx @vscode/vsce package` in `editors/vscode` makes a
   `nyra-0.5.0.vsix` that `code --install-extension nyra-0.5.0.vsix` installs.)
3. If `nyra` is not on `PATH`, set `nyra.path` in the settings to the executable, for example
   `"nyra.path": "C:\\tools\\nyra.exe"`.

To try it without installing anything, start a separate VS Code with the extension loaded from the folder:

```bash
code --extensionDevelopmentPath="$PWD/editors/vscode" examples
```

Settings:

| Setting | Default | |
| --- | --- | --- |
| `nyra.path` | `nyra` | the executable that runs `nyra lsp` |
| `nyra.lsp.enabled` | `true` | start the language server (off: only highlighting and the icon) |
| `nyra.formatOnSave` | `false` | format `.nyra` files on save (VS Code's `editor.formatOnSave` works too) |

The command **Nyra: Restart the language server** starts a new server, for example after you rebuilt `nyra`. The
server's own messages are in the **Nyra** output channel.

## Other editors

Any LSP client works: the command is `nyra lsp`, the files are `*.nyra`, the language id is `nyra`.

**Neovim** (0.11 or later), in `init.lua`:

```lua
vim.filetype.add({ extension = { nyra = "nyra" } })
vim.lsp.config("nyra", { cmd = { "nyra", "lsp" }, filetypes = { "nyra" }, root_markers = { ".git" } })
vim.lsp.enable("nyra")
```

**Helix**, in `languages.toml`:

```toml
[language-server.nyra]
command = "nyra"
args = ["lsp"]

[[language]]
name = "nyra"
scope = "source.nyra"
file-types = ["nyra"]
comment-token = "//"
indent = { tab-width = 4, unit = "    " }
language-servers = ["nyra"]
```

**Emacs** (Eglot), with a `nyra-mode` for the files:

```elisp
(add-to-list 'eglot-server-programs '(nyra-mode "nyra" "lsp"))
```

## The protocol, for client authors

- Transport: stdio, `Content-Length` headers. `nyra lsp --stdio` is accepted too.
- Position encoding: UTF-16 columns, or UTF-32 when the client offers `utf-32` in
  `general.positionEncodings`.
- Text sync: full documents (`change: 1`); a client that sends ranges anyway is understood.
- `textDocument/publishDiagnostics`: `severity` 1 for errors and 2 for warnings, `code` (`"E0201"`),
  `codeDescription.href`, `source: "nyra"`, the hint in the message. A mistake inside an imported file is shown on
  the importer's `use` line, with `relatedInformation` pointing into that file.
- Requests: `hover`, `completion` (trigger character `.`), `signatureHelp` (`(` and `,`), `definition`,
  `documentSymbol` (hierarchical), `formatting` (one edit of the whole text), `codeAction` (kinds `quickfix` and
  `source.fixAll`).
- A panic of the compiler while answering becomes an error reply; the server keeps running.

## Limits

- Not yet: rename, find references, workspace symbols, semantic highlighting, inlay hints.
- Names are found from the tokens, so they work in a file with errors: a variable is the closest declaration of
  that name before it whose block is still open. Types come from the checker; while the line being typed does not
  parse, hover and completion use the last version that did.
- The checks that need a site build (`nyra check site.nyra`: anchors, images, heading order, contrast) are not run
  by the server; the compile errors and warnings of a `use web` program are.
- The `ex` examples run in every check, with the same limits as in `nyra check`.
