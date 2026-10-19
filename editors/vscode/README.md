# Nyra for VS Code

Syntax highlighting for `.nyra` files, including expressions inside `{ }` in strings, and the Nyra icon for
`.nyra` files in the explorer and on editor tabs. Semicolons are highlighted as errors.

VS Code uses the language's icon when the file icon theme has none of its own for `.nyra`; a theme that draws
every file itself (such as Material Icon Theme) keeps its own icon.

## Install (from this repo)

Copy this folder into your VS Code extensions folder and reload VS Code:

```bash
cp -r editors/vscode ~/.vscode/extensions/spakxd.nyra-0.4.0
```

On Windows the folder is `%USERPROFILE%\.vscode\extensions\`. Remove an older `spakxd.nyra-*` folder first.

For the icon of `.nyra` files in Windows Explorer, see `tools/windows/install-file-icon.ps1`.
