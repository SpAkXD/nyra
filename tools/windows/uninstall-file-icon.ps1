<#
.SYNOPSIS
    Removes what install-file-icon.ps1 added: the Nyra.Source ProgID, the .nyra entry it made, and the icon in
    %LOCALAPPDATA%\Nyra. A ProgID that .nyra had before is restored.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File tools\windows\uninstall-file-icon.ps1
#>
$ErrorActionPreference = 'Stop'
$ProgId = 'Nyra.Source'
$Classes = 'HKCU:\Software\Classes'
$extKey = "$Classes\.nyra"

Add-Type -Namespace NyraIcon -Name ShellNotify -MemberDefinition @'
[DllImport("shell32.dll")]
public static extern void SHChangeNotify(int eventId, uint flags, System.IntPtr item1, System.IntPtr item2);
'@

if (Test-Path -LiteralPath $extKey) {
    $ext = Get-Item -LiteralPath $extKey
    $previous = $ext.GetValue('Nyra.PreviousProgId')
    if ($ext.GetValue('') -eq $ProgId) {
        if ($previous) {
            Set-Item -LiteralPath $extKey -Value $previous
            Remove-ItemProperty -LiteralPath $extKey -Name 'Nyra.PreviousProgId'
            if (Test-Path -LiteralPath "$extKey\OpenWithProgids") {
                Remove-ItemProperty -LiteralPath "$extKey\OpenWithProgids" -Name $previous -ErrorAction SilentlyContinue
            }
            Write-Host "Restored .nyra -> $previous"
        } else {
            # the key was made by the installer: remove it, keeping anything else other tools put there
            $others = @($ext.GetValueNames() | Where-Object { $_ -notin @('', 'PerceivedType', 'Content Type') })
            $subkeys = @($ext.GetSubKeyNames() | Where-Object { $_ -ne 'OpenWithProgids' })
            if ($others.Count -eq 0 -and $subkeys.Count -eq 0) {
                Remove-Item -LiteralPath $extKey -Recurse -Force
            } else {
                $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\Classes\.nyra', $true)
                $key.DeleteValue('', $false)
                $key.Close()
            }
        }
    }
}

if (Test-Path -LiteralPath "$Classes\$ProgId") { Remove-Item -LiteralPath "$Classes\$ProgId" -Recurse -Force }

$icon = Join-Path $env:LOCALAPPDATA 'Nyra\nyra.ico'
if (Test-Path -LiteralPath $icon) { Remove-Item -LiteralPath $icon -Force }
$dir = Join-Path $env:LOCALAPPDATA 'Nyra'
if ((Test-Path -LiteralPath $dir) -and -not (Get-ChildItem -LiteralPath $dir -Force)) { Remove-Item -LiteralPath $dir -Force }

[NyraIcon.ShellNotify]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)
Write-Host 'Removed the Nyra file icon.'
