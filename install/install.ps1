<#
.SYNOPSIS
    Installs Nyra (https://nyralang.dev) on Windows, for the current user only (no administrator rights).

.DESCRIPTION
    1. downloads the release zip for x86_64 Windows and SHA256SUMS.txt from GitHub and checks the zip's SHA-256,
    2. puts nyra.exe in %LOCALAPPDATA%\Nyra\bin,
    3. adds that folder to your user PATH (HKCU\Environment) and tells running programs about it,
    4. gives .nyra files the Nyra icon in Explorer (-FileIcon, on by default),
    5. checks for a C compiler (only native programs need one).
    Running it again installs the requested version over the old one; nothing is added twice.

.EXAMPLE
    irm https://nyralang.dev/install.ps1 | iex

.EXAMPLE
    & ([scriptblock]::Create((irm https://nyralang.dev/install.ps1))) -Version 0.7.0

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File install\install.ps1 -Uninstall
#>
param(
    # The version to install (0.7.0 or v0.7.0); default: the latest release. Also read from $env:NYRA_VERSION.
    [string]$Version = $env:NYRA_VERSION,
    # Where Nyra goes; nyra.exe is put in its bin folder.
    [string]$InstallDir = $(if ($env:NYRA_HOME) { $env:NYRA_HOME } else { Join-Path $env:LOCALAPPDATA 'Nyra' }),
    # Give .nyra files the Nyra icon in Explorer (-FileIcon:$false or -NoFileIcon to skip).
    [switch]$FileIcon = $true,
    [switch]$NoFileIcon,
    # Do not change the user PATH.
    [switch]$NoPath,
    # Download, check and unpack into InstallDir, but change nothing else (PATH, file icon): print what would change.
    [switch]$DryRun,
    # Remove nyra.exe, the PATH entry and the file icon.
    [switch]$Uninstall,
    # The GitHub repository to download from.
    [string]$Repo = $(if ($env:NYRA_REPO) { $env:NYRA_REPO } else { 'SpAkXD/nyra' })
)

# Everything runs in a function, so `irm ... | iex` leaves no variables or settings behind in your session, and an
# error stops the installer without closing the window.
function Install-Nyra {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'   # Windows PowerShell 5.1 downloads far faster without a progress bar
    $bin = Join-Path $InstallDir 'bin'
    $exe = Join-Path $bin 'nyra.exe'
    $doIcon = $FileIcon -and -not $NoFileIcon

    if ($Uninstall) { Uninstall-Nyra $bin $exe; return }

    # --- which build ---------------------------------------------------------------------------
    $arch = $env:PROCESSOR_ARCHITECTURE
    if ($env:PROCESSOR_ARCHITEW6432) { $arch = $env:PROCESSOR_ARCHITEW6432 }  # a 32-bit PowerShell on 64-bit Windows
    if ($arch -eq 'ARM64') {
        Write-Host 'Note: there is no ARM64 build yet; the x86_64 build runs under Windows 11 emulation.'
    } elseif ($arch -ne 'AMD64') {
        throw "nyra install: there is only an x86_64 build for Windows (this is $arch). Build it from source: install Rust (https://rustup.rs), then run: cargo install --git https://github.com/$Repo"
    }
    $target = 'x86_64-pc-windows-msvc'

    # PowerShell 5.1 may still default to TLS 1.0, which GitHub refuses
    try { [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12 } catch {}

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("nyra-install-" + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tmp -Force | Out-Null
    try {
        # --- the version -----------------------------------------------------------------------
        if (-not $Version) {
            try {
                $latest = Invoke-RestMethod -UseBasicParsing -Uri "https://api.github.com/repos/$Repo/releases/latest" -Headers @{ 'User-Agent' = 'nyra-install' }
            } catch {
                throw "nyra install: cannot ask GitHub for the latest release ($($_.Exception.Message)). Pass -Version X to skip this."
            }
            $Version = $latest.tag_name
        }
        $tag = if ($Version.StartsWith('v')) { $Version } else { "v$Version" }
        $asset = "nyra-$tag-$target.zip"
        $base = "https://github.com/$Repo/releases/download/$tag"
        Write-Host "Installing Nyra $tag ($target) into $bin"

        # --- download and check ----------------------------------------------------------------
        $zip = Join-Path $tmp $asset
        $sums = Join-Path $tmp 'SHA256SUMS.txt'
        try { Invoke-WebRequest -UseBasicParsing -Uri "$base/$asset" -OutFile $zip } catch {
            throw "nyra install: cannot download $base/$asset ($($_.Exception.Message)). Is $tag a release of ${Repo}?"
        }
        try { Invoke-WebRequest -UseBasicParsing -Uri "$base/SHA256SUMS.txt" -OutFile $sums } catch {
            throw "nyra install: cannot download $base/SHA256SUMS.txt ($($_.Exception.Message))"
        }
        $expected = $null
        foreach ($line in Get-Content -LiteralPath $sums) {
            $parts = $line.Trim() -split '\s+', 2
            if ($parts.Count -eq 2 -and $parts[1].TrimStart('*') -eq $asset) { $expected = $parts[0].ToLowerInvariant() }
        }
        if (-not $expected) { throw "nyra install: SHA256SUMS.txt of $tag has no line for $asset" }
        $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $zip).Hash.ToLowerInvariant()
        if ($actual -ne $expected) {
            throw "nyra install: the SHA-256 of $asset is $actual, but SHA256SUMS.txt says $expected. Nothing was installed."
        }
        Write-Host "Checked the SHA-256 of $asset"

        # --- unpack and install ----------------------------------------------------------------
        $unpacked = Join-Path $tmp 'x'
        Expand-Archive -LiteralPath $zip -DestinationPath $unpacked -Force
        $root = Join-Path $unpacked "nyra-$tag-$target"
        $newExe = Join-Path $root 'nyra.exe'
        if (-not (Test-Path -LiteralPath $newExe)) { throw "nyra install: the zip has no nyra-$tag-$target\nyra.exe" }

        New-Item -ItemType Directory -Path $bin -Force | Out-Null
        if (Test-Path -LiteralPath $exe) {
            # a running nyra.exe cannot be overwritten, but it can be renamed
            $old = "$exe.old"
            Remove-Item -LiteralPath $old -Force -ErrorAction SilentlyContinue
            if (Test-Path -LiteralPath $old) { $old = "$exe.old-$PID" }
            Move-Item -LiteralPath $exe -Destination $old -Force
            Remove-Item -LiteralPath $old -Force -ErrorAction SilentlyContinue  # fails only while it runs; then it stays
        }
        Copy-Item -LiteralPath $newExe -Destination $exe -Force
        Write-Host "Installed $exe"
        # (nyra prints its version on stderr up to v0.8; PowerShell 5.1 would turn that into an error under 'Stop')
        $ErrorActionPreference = 'Continue'
        $ver = ((& $exe --version 2>&1 | ForEach-Object { "$_" }) -join ' ').Trim()
        $ErrorActionPreference = 'Stop'
        if ($ver -like 'nyra *') { Write-Host $ver } else { Write-Warning "$exe did not start: $ver" }

        # the file-icon scripts: in the zip since v0.9; for older releases from the repository
        $iconScript = Join-Path $InstallDir 'tools\windows\install-file-icon.ps1'
        if ($doIcon) { Save-IconFiles $root $tag (Join-Path $tmp 'icon') }
    } finally {
        Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }

    # --- PATH ----------------------------------------------------------------------------------
    if ($NoPath) {
        if (-not (Test-OnPath $bin)) { Write-Host "PATH: not changed (-NoPath). Add $bin to your PATH to run nyra by name." }
    } else {
        $current = Get-UserPath
        $updated = Add-PathEntry $current $bin
        if ($updated -eq $current) {
            Write-Host "PATH: your user PATH already has $bin"
        } elseif ($DryRun) {
            Write-Host "PATH (dry run): would add $bin to your user PATH"
        } else {
            Set-UserPath $updated
            Write-Host "PATH: added $bin to your user PATH (open a new terminal to use it)"
        }
        if (-not $DryRun -and -not (Test-OnPath $bin)) { $env:Path = "$bin;$env:Path" }  # this session too
    }

    # --- file icon -----------------------------------------------------------------------------
    if ($doIcon) {
        if (-not (Test-Path -LiteralPath $iconScript)) {
            Write-Host 'File icon: skipped (this release has no icon script; run the installer of a newer release).'
        } elseif ($DryRun) {
            Write-Host "File icon (dry run): would run $iconScript"
        } else {
            & powershell -NoProfile -ExecutionPolicy Bypass -File $iconScript
            if ($LASTEXITCODE -ne 0) { Write-Warning 'The file icon could not be registered (Nyra itself is installed).' }
        }
    }

    # --- a C compiler --------------------------------------------------------------------------
    $cc = $null
    foreach ($name in @($env:NYRA_CC, 'gcc', 'clang', 'cc', 'tcc')) {
        if ($name -and (Get-Command $name -ErrorAction SilentlyContinue)) { $cc = $name; break }
    }
    if ($cc) {
        Write-Host "C compiler: $cc (for native programs: nyra build, nyra run --native)"
    } else {
        Write-Host 'C compiler: none found. The interpreter (nyra run) and the JavaScript target (--js) work without one;'
        Write-Host '            native programs (nyra build) need one. The simplest is GCC from MSYS2:'
        Write-Host '              winget install -e --id MSYS2.MSYS2'
        Write-Host '              C:\msys64\usr\bin\bash -lc "pacman -S --noconfirm mingw-w64-ucrt-x86_64-gcc"'
        Write-Host '              then add C:\msys64\ucrt64\bin to your PATH'
        Write-Host '            or Clang: winget install -e --id LLVM.LLVM (it links with the Visual Studio Build Tools).'
    }

    Write-Host ''
    Write-Host 'Start a project:  nyra new script hello; cd hello; nyra run main.nyra'
    Write-Host 'Docs:             https://nyralang.dev  (upgrade later with: nyra upgrade)'
}

# Copies install-file-icon.ps1, uninstall-file-icon.ps1 and nyra.ico into $InstallDir, keeping the repository's
# layout (tools\windows and assets), which the scripts expect.
function Save-IconFiles([string]$root, [string]$tag, [string]$stage) {
    $files = @('tools\windows\install-file-icon.ps1', 'tools\windows\uninstall-file-icon.ps1', 'assets\nyra.ico')
    # gather all three first (without every one the icon script is useless), then copy them in
    foreach ($f in $files) {
        $staged = Join-Path $stage $f
        New-Item -ItemType Directory -Path (Split-Path $staged) -Force | Out-Null
        $inZip = Join-Path $root $f
        if (Test-Path -LiteralPath $inZip) { Copy-Item -LiteralPath $inZip -Destination $staged -Force; continue }
        $saved = $false
        foreach ($ref in @($tag, 'main')) {
            try {
                Invoke-WebRequest -UseBasicParsing -Uri "https://raw.githubusercontent.com/$Repo/$ref/$($f -replace '\\', '/')" -OutFile $staged
                $saved = $true
                break
            } catch {}
        }
        if (-not $saved) { return }
    }
    foreach ($f in $files) {
        $dest = Join-Path $InstallDir $f
        New-Item -ItemType Directory -Path (Split-Path $dest) -Force | Out-Null
        Copy-Item -LiteralPath (Join-Path $stage $f) -Destination $dest -Force
    }
}

function Get-UserPath {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment')
    if (-not $key) { return '' }
    try { return [string]$key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) } finally { $key.Close() }
}

function Set-UserPath([string]$value) {
    $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment')
    try { $key.SetValue('Path', $value, [Microsoft.Win32.RegistryValueKind]::ExpandString) } finally { $key.Close() }
    # tell Explorer and other running programs that the environment changed (WM_SETTINGCHANGE, "Environment")
    if (-not ('NyraInstall.Native' -as [type])) {
        Add-Type -Namespace NyraInstall -Name Native -MemberDefinition @'
[DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
public static extern System.IntPtr SendMessageTimeout(System.IntPtr hWnd, uint msg, System.UIntPtr wParam, string lParam, uint flags, uint timeout, out System.UIntPtr result);
'@
    }
    $result = [UIntPtr]::Zero
    [void][NyraInstall.Native]::SendMessageTimeout([IntPtr]0xffff, 0x1A, [UIntPtr]::Zero, 'Environment', 2, 5000, [ref]$result)
}

function Get-DirKey([string]$dir) { return [Environment]::ExpandEnvironmentVariables($dir).TrimEnd('\').ToLowerInvariant() }

# The PATH string with $dir at its end, or unchanged when it already has it.
function Add-PathEntry([string]$path, [string]$dir) {
    $want = Get-DirKey $dir
    foreach ($p in $path -split ';') { if ($p -and (Get-DirKey $p) -eq $want) { return $path } }
    if (-not $path) { return $dir }
    return $path.TrimEnd(';') + ';' + $dir
}

# The PATH string without $dir.
function Remove-PathEntry([string]$path, [string]$dir) {
    $want = Get-DirKey $dir
    $kept = @($path -split ';' | Where-Object { $_ -and (Get-DirKey $_) -ne $want })
    return $kept -join ';'
}

function Test-OnPath([string]$dir) {
    $want = Get-DirKey $dir
    foreach ($p in $env:Path -split ';') { if ($p -and (Get-DirKey $p) -eq $want) { return $true } }
    return $false
}

function Uninstall-Nyra([string]$bin, [string]$exe) {
    $ErrorActionPreference = 'Stop'
    $what = if ($DryRun) { 'would remove' } else { 'removed' }
    # the file icon first: its uninstall script lives in the folder that goes next
    $iconUndo = Join-Path $InstallDir 'tools\windows\uninstall-file-icon.ps1'
    if (-not $NoFileIcon -and (Test-Path -LiteralPath $iconUndo)) {
        if ($DryRun) { Write-Host "File icon (dry run): would run $iconUndo" }
        else { & powershell -NoProfile -ExecutionPolicy Bypass -File $iconUndo }
    }
    foreach ($f in @($exe, "$exe.old") + @(Get-ChildItem -LiteralPath $bin -Filter 'nyra.exe.old-*' -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })) {
        if (Test-Path -LiteralPath $f) {
            if (-not $DryRun) {
                try { Remove-Item -LiteralPath $f -Force } catch { throw "nyra install: cannot remove $f (is nyra still running?): $($_.Exception.Message)" }
            }
            Write-Host "$what $f"
        }
    }
    if (-not $DryRun) {
        foreach ($d in @($bin, (Join-Path $InstallDir 'tools'), (Join-Path $InstallDir 'assets'))) {
            if (Test-Path -LiteralPath $d) { Remove-Item -LiteralPath $d -Recurse -Force }
        }
        Remove-Item -LiteralPath (Join-Path $InstallDir 'nyra.ico') -Force -ErrorAction SilentlyContinue
        if ((Test-Path -LiteralPath $InstallDir) -and -not (Get-ChildItem -LiteralPath $InstallDir -Force)) { Remove-Item -LiteralPath $InstallDir -Force }
    }
    if (-not $NoPath) {
        $current = Get-UserPath
        $updated = Remove-PathEntry $current $bin
        if ($updated -ne $current) {
            if ($DryRun) { Write-Host "PATH (dry run): would remove $bin from your user PATH" }
            else { Set-UserPath $updated; Write-Host "PATH: removed $bin from your user PATH" }
        }
    }
    Write-Host 'Nyra is uninstalled.'
}

try {
    Install-Nyra
} catch {
    Write-Host $_.Exception.Message -ForegroundColor Red
    # run as a file (powershell -File install.ps1): fail with exit code 1; through `irm | iex` an exit would close
    # the user's window, so it only reports the error
    if ($MyInvocation.MyCommand.Path) { exit 1 }
}
