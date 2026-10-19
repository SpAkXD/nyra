# Installing Nyra

Nyra is one executable, `nyra`, with no dependencies. Release builds exist for:

| Platform | Archive |
|---|---|
| Windows x86_64 (also Windows 11 on ARM, through its x64 emulation) | `nyra-vX.Y.Z-x86_64-pc-windows-msvc.zip` |
| Linux x86_64 (glibc) | `nyra-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz` |
| macOS on Apple Silicon | `nyra-vX.Y.Z-aarch64-apple-darwin.tar.gz` |

Anything else (Intel Macs, ARM Linux, musl, the BSDs) builds from source in about a minute; see
[From source](#from-source).

## Windows

In PowerShell (no administrator rights needed):

```powershell
irm https://nyralang.dev/install.ps1 | iex
```

The script:

1. downloads the newest release zip and `SHA256SUMS.txt` from GitHub and checks the zip's SHA-256,
2. puts `nyra.exe` in `%LOCALAPPDATA%\Nyra\bin`,
3. adds that folder to your user PATH (`HKCU\Environment`) and tells running programs about it,
4. gives `.nyra` files the Nyra icon in Explorer (see [The file icon](#the-file-icon-windows)),
5. checks for a C compiler and says how to get one if there is none.

Open a new terminal afterwards so it sees the new PATH. Running the script again installs over the old
version; nothing is added twice.

Options (pass them through a script block, since `iex` takes none):

```powershell
& ([scriptblock]::Create((irm https://nyralang.dev/install.ps1))) -Version 0.9.0 -NoFileIcon
```

| Option | Meaning |
|---|---|
| `-Version X` | install version X instead of the newest (also `$env:NYRA_VERSION`) |
| `-InstallDir DIR` | install into `DIR\bin` instead of `%LOCALAPPDATA%\Nyra\bin` (also `$env:NYRA_HOME`) |
| `-NoFileIcon` | do not register the `.nyra` file icon |
| `-NoPath` | do not change the user PATH |
| `-DryRun` | download, check and unpack into the install folder, but change nothing else; print what would change |
| `-Uninstall` | remove Nyra (see [Uninstall](#uninstall)) |

From a checkout of the repository: `powershell -ExecutionPolicy Bypass -File install\install.ps1`.

### The file icon (Windows)

`-FileIcon` (on by default) runs `tools\windows\install-file-icon.ps1` from the release: `.nyra` files get the
Nyra icon and the type name "Nyra source file", for the current user only. The editor you already open
`.nyra` files with stays the same. Releases before v0.9 do not ship the script; the installer then fetches it
from the repository, or skips the icon and says so.

## macOS and Linux

```sh
curl -fsSL https://nyralang.dev/install.sh | sh
```

The script:

1. picks the archive for this computer and stops with a clear message on a platform without a release build,
2. downloads it and `SHA256SUMS.txt` (with `curl` or `wget`) and checks the SHA-256 (`sha256sum` or
   `shasum -a 256`),
3. puts `nyra` in `~/.nyra/bin` (or `$NYRA_HOME/bin`),
4. adds one marked line to your shell's startup file: `~/.zshrc` for zsh, `~/.bashrc` for bash on Linux,
   `~/.bash_profile` for bash on macOS, `~/.config/fish/conf.d/nyra.fish` for fish, `~/.profile` otherwise,
5. checks for a C compiler.

Open a new terminal afterwards, or run the `export PATH=...` line it prints. Running it again installs over
the old version and does not add the PATH line twice. It also works in Git Bash, MSYS2 and Cygwin on Windows
(it installs `nyra.exe` and edits `~/.bashrc`).

Options (after `sh -s --` when piped):

```sh
curl -fsSL https://nyralang.dev/install.sh | sh -s -- --version 0.9.0 --no-path
```

| Option | Meaning |
|---|---|
| `--version X` | install version X instead of the newest (also `NYRA_VERSION=X`) |
| `--no-path` | do not change any shell startup file |
| `--uninstall` | remove Nyra (see [Uninstall](#uninstall)) |
| `NYRA_HOME=DIR` | install into `DIR/bin` instead of `~/.nyra/bin` |

The scripts are also in the repository (`install/install.sh`, `install/install.ps1`) and attached to every
release from v0.9 on, so `https://github.com/SpAkXD/nyra/releases/latest/download/install.sh` works too.

## By hand

1. Download the archive for your platform and `SHA256SUMS.txt` from
   [the releases page](https://github.com/SpAkXD/nyra/releases).
2. Check it: `sha256sum -c SHA256SUMS.txt --ignore-missing` (Linux), `shasum -a 256 -c SHA256SUMS.txt
   --ignore-missing` (macOS), or `Get-FileHash nyra-*.zip` in PowerShell and compare.
3. Unpack it and put `nyra` (`nyra.exe`) in a folder on your PATH. The archive also has `docs/` and `examples/`.

## Package managers

Each release from v0.9 on carries a filled Homebrew formula (`nyra.rb`) and winget manifests
(`winget-SpAkXD.Nyra*.yaml`), made by `install/packaging/fill.sh` from the templates in `install/packaging/`.
They are not published in Homebrew or winget yet. Until they are, use the install scripts. When they are:

```sh
brew install SpAkXD/tap/nyra       # macOS (Apple Silicon) and Linux x86_64
winget install SpAkXD.Nyra         # Windows
```

## From source

Nyra has no crate dependencies; any Rust toolchain (https://rustup.rs) builds it:

```sh
cargo install --git https://github.com/SpAkXD/nyra
```

or clone the repository and run `cargo build --release` (the executable is `target/release/nyra`).

## Check it

```sh
nyra --version
nyra new script hello
cd hello
nyra run main.nyra
```

`nyra new` starts a project: `script`, `cli`, `site` or `lib` (run `nyra new` for the list). Each has a
README.md with its commands and an AGENTS.md for AI agents.

## A C compiler

The interpreter (`nyra run`, `--interp`, `--sandbox`) and the JavaScript target (`--js`) need nothing else.
Native programs (`nyra build`, `nyra run --native`, and `nyra run` of a long-running program or one that uses
`fs` or `os`) need a C compiler: `gcc`, `clang`, `cc` or `tcc` on the PATH, or the one named in `NYRA_CC`.
Without one, `nyra run` falls back to the interpreter and `nyra build` says what is missing.

| System | How |
|---|---|
| macOS | `xcode-select --install` |
| Debian, Ubuntu | `sudo apt install gcc` |
| Fedora | `sudo dnf install gcc` |
| Arch | `sudo pacman -S gcc` |
| Alpine | `sudo apk add gcc musl-dev` |
| Windows | `winget install -e --id MSYS2.MSYS2`, then in the MSYS2 shell `pacman -S mingw-w64-ucrt-x86_64-gcc`, and add `C:\msys64\ucrt64\bin` to your PATH |
| Windows (Clang) | `winget install -e --id LLVM.LLVM`; it links with the Visual Studio Build Tools, which must be installed too |

## Upgrade

```sh
nyra upgrade            # install the newest release over this nyra
nyra upgrade --check    # only say whether there is a newer one
nyra upgrade --version 0.8.0    # a given version, newer or older
```

`nyra upgrade` asks GitHub for the newest release (with the system's `curl`), downloads the archive for this
platform, checks its SHA-256 against the release's `SHA256SUMS.txt`, checks that the new executable starts,
and only then replaces the running one. On Windows the running `nyra.exe` is renamed to `nyra.exe.old` first
(Windows cannot overwrite a running program); the next `nyra upgrade` deletes it. A nyra installed by
Homebrew, winget or cargo is left to that tool, and `nyra upgrade` says which command to run.

It needs `curl` and `tar`, which come with Windows 10 and later, macOS and Linux. Running an install script
again also upgrades.

## Uninstall

| How it was installed | Remove it with |
|---|---|
| `install.ps1` | `& ([scriptblock]::Create((irm https://nyralang.dev/install.ps1))) -Uninstall` (removes `nyra.exe`, the PATH entry and the file icon) |
| `install.sh` | `curl -fsSL https://nyralang.dev/install.sh \| sh -s -- --uninstall` (removes `nyra` and the PATH line) |
| cargo | `cargo uninstall nyra` |
| by hand | delete the executable |

Your projects and the build cache in the system's temp folder (`nyra/` there) are not touched; delete the
cache folder if you want the space back.

## Troubleshooting

- **`nyra` is not found after installing.** The PATH change applies to terminals opened afterwards. Open a
  new one, or use the full path (`%LOCALAPPDATA%\Nyra\bin\nyra.exe`, `~/.nyra/bin/nyra`). On Windows, an
  editor's built-in terminal may need the editor restarted.
- **"cannot ask GitHub for the latest release".** You are offline, behind a proxy, or over GitHub's limit of
  60 API requests an hour per address. Pass a version (`-Version 0.9.0`, `--version 0.9.0`), which skips the
  API. `curl` honors `HTTPS_PROXY`.
- **"the SHA-256 of ... is ..., but SHA256SUMS.txt says ...".** The download was damaged or changed on the
  way. Nothing was installed. Try again; if it repeats, open an issue.
- **PowerShell refuses to run the script** ("running scripts is disabled"). Use `irm ... | iex` as shown, or
  `powershell -ExecutionPolicy Bypass -File install.ps1`.
- **Linux: the new `nyra` does not start** (a `GLIBC_...` error). The release is built on a recent Ubuntu; on an
  older distribution build from source.
- **`nyra upgrade`: "permission denied".** nyra lives in a folder you cannot write to (for example
  `/usr/local/bin`). Run it as the user who installed it, or reinstall with the install script into your home.
- **Windows: a `nyra.exe.old` next to `nyra.exe`.** Left by an upgrade while that nyra was running; the next
  `nyra upgrade` removes it, or delete it.
- **The `.nyra` icon does not show.** If you once chose "Always use this app" for `.nyra`, Windows shows that
  app's icon. Run `%LOCALAPPDATA%\Nyra\tools\windows\install-file-icon.ps1 -ResetChoice`.
- **`nyra build`: "no C compiler found".** See [A C compiler](#a-c-compiler); `nyra run --interp` and
  `--js` work meanwhile.
