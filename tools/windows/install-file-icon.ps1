<#
.SYNOPSIS
    Gives .nyra files the Nyra icon and the type name "Nyra source file" in Windows Explorer.

.DESCRIPTION
    Per user (HKCU\Software\Classes), no administrator rights. It:
      - copies the icon to %LOCALAPPDATA%\Nyra\nyra.ico,
      - registers the ProgID Nyra.Source (DefaultIcon, FriendlyTypeName, PerceivedType text) and points .nyra at it,
      - keeps the editor you already open .nyra files with: its open command is copied into Nyra.Source, and an
        earlier ProgID stays in "Open with". With no editor yet, Windows asks as before. It never picks a program
        for you,
      - refreshes Explorer's icons.
    Undo it with uninstall-file-icon.ps1.

    If you once chose "Always use this app" for .nyra, Windows takes the icon from that choice. The script then says
    so; run it again with -ResetChoice to clear that choice (the same editor still opens the files, through
    Nyra.Source).

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File tools\windows\install-file-icon.ps1
#>
param(
    # The icon to install; default: assets\nyra.ico of this repository.
    [string]$IconPath = (Join-Path $PSScriptRoot '..\..\assets\nyra.ico'),
    # Clear an "Always use this app" choice for .nyra so the Nyra icon shows.
    [switch]$ResetChoice
)

$ErrorActionPreference = 'Stop'
$ProgId = 'Nyra.Source'
$Classes = 'HKCU:\Software\Classes'
$FileExts = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.nyra'

Add-Type -Namespace NyraIcon -Name Shell -MemberDefinition @'
[DllImport("shell32.dll")]
public static extern void SHChangeNotify(int eventId, uint flags, System.IntPtr item1, System.IntPtr item2);
[DllImport("shlwapi.dll", CharSet = CharSet.Unicode)]
public static extern int AssocQueryString(int flags, int str, string assoc, string extra, System.Text.StringBuilder outBuf, ref uint outLen);
'@

function Get-AssocCommand([string]$assoc, [int]$flags = 0x20) {
    # ASSOCSTR_COMMAND = 1, ASSOCF_NOTRUNCATE = 0x20; what double-clicking a file runs today, or $null
    $len = [uint32]2048
    $sb = New-Object System.Text.StringBuilder 2048
    if ([NyraIcon.Shell]::AssocQueryString($flags, 1, $assoc, 'open', $sb, [ref]$len) -eq 0) { return $sb.ToString() }
    return $null
}

function Get-Default([string]$key) {
    if (Test-Path -LiteralPath $key) { return (Get-Item -LiteralPath $key).GetValue('') }
    return $null
}

function Set-Default([string]$key, [string]$value) {
    if (-not (Test-Path -LiteralPath $key)) { New-Item -Path $key -Force | Out-Null }
    Set-Item -LiteralPath $key -Value $value
}

# 1. the icon
$IconPath = (Resolve-Path -LiteralPath $IconPath).Path
$dir = Join-Path $env:LOCALAPPDATA 'Nyra'
New-Item -ItemType Directory -Path $dir -Force | Out-Null
$icon = Join-Path $dir 'nyra.ico'
Copy-Item -LiteralPath $IconPath -Destination $icon -Force

# 2. what opens .nyra files now (before anything changes)
$extKey = "$Classes\.nyra"
$previous = Get-Default $extKey
if ($previous -eq $ProgId) { $previous = (Get-Item -LiteralPath $extKey).GetValue('Nyra.PreviousProgId') }
$command = $null
$choice = $null
if (Test-Path -LiteralPath "$FileExts\UserChoice") { $choice = (Get-Item -LiteralPath "$FileExts\UserChoice").GetValue('ProgId') }
if ($choice -ne $ProgId) { $command = Get-AssocCommand '.nyra' }
if ($command -and $command -like '*OpenWith.exe*') { $command = $null }  # Windows' own "choose an app" dialog
if (-not $command -and (Test-Path -LiteralPath "$FileExts\OpenWithList")) {
    # no default program, but files were opened with "Open with": take the most recent app (Code.exe, ...)
    $list = Get-Item -LiteralPath "$FileExts\OpenWithList"
    $mru = $list.GetValue('MRUList')
    if ($mru) {
        $app = $list.GetValue([string]$mru[0])
        if ($app) { $command = Get-AssocCommand $app 0x22 }  # ASSOCF_INIT_BYEXENAME: an app, not an extension
    }
}

# 3. the ProgID
$progKey = "$Classes\$ProgId"
Set-Default $progKey 'Nyra source file'
New-ItemProperty -LiteralPath $progKey -Name 'FriendlyTypeName' -Value 'Nyra source file' -PropertyType String -Force | Out-Null
Set-Default "$progKey\DefaultIcon" "`"$icon`",0"
if ($command) {
    Set-Default "$progKey\shell\open\command" $command
}

# 4. the extension
Set-Default $extKey $ProgId
if ($previous -and $previous -ne $ProgId) {
    New-ItemProperty -LiteralPath $extKey -Name 'Nyra.PreviousProgId' -Value $previous -PropertyType String -Force | Out-Null
    # keep the earlier ProgID in "Open with" (an empty REG_NONE value, as Windows writes them)
    $owp = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Software\Classes\.nyra\OpenWithProgids')
    $owp.SetValue($previous, [byte[]]@(), [Microsoft.Win32.RegistryValueKind]::None)
    $owp.Close()
}
New-ItemProperty -LiteralPath $extKey -Name 'PerceivedType' -Value 'text' -PropertyType String -Force | Out-Null
New-ItemProperty -LiteralPath $extKey -Name 'Content Type' -Value 'text/plain' -PropertyType String -Force | Out-Null

# 5. an "Always use this app" choice wins over the ProgID, icon included
if ($choice -and $choice -ne $ProgId) {
    if ($ResetChoice) {
        Remove-Item -LiteralPath "$FileExts\UserChoice" -Force
        Write-Host "Cleared the 'Always use this app' choice ($choice) for .nyra."
    } else {
        Write-Host "Note: .nyra files are set to always open with '$choice', and Windows shows that app's icon."
        Write-Host "      Run this script again with -ResetChoice to show the Nyra icon (the same editor keeps opening them)."
    }
}

# 6. refresh Explorer (SHCNE_ASSOCCHANGED, SHCNF_IDLIST)
[NyraIcon.Shell]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)

Write-Host "Installed the Nyra file icon: $icon"
if ($command) { Write-Host "Double-click opens: $command" } else { Write-Host "No editor was set for .nyra: Windows asks which app to use, as before." }
