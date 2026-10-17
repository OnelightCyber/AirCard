<#
.SYNOPSIS
    Installs AirCard for the current user (no administrator rights required).

.DESCRIPTION
    - Uses target\release\aircard.exe if present, otherwise builds it with cargo.
    - Copies the executable to %LOCALAPPDATA%\Programs\AirCard.
    - Creates Desktop and Start Menu shortcuts.
    - Checks the Apple Mobile Device Support DLL prerequisite and, if missing,
      offers to open the official 64-bit iTunes download page.

.PARAMETER Uninstall
    Removes the installed files and shortcuts.

.PARAMETER NoShortcut
    Skips creating the Desktop / Start Menu shortcuts.

.PARAMETER NoLaunch
    Does not launch AirCard after installing.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File .\install.ps1

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File .\install.ps1 -Uninstall
#>
[CmdletBinding()]
param(
    [switch]$Uninstall,
    [switch]$NoShortcut,
    [switch]$NoLaunch
)

$ErrorActionPreference = 'Stop'

$AppName     = 'AirCard'
$RepoRoot    = Split-Path -Parent $MyInvocation.MyCommand.Path
$InstallDir  = Join-Path $env:LOCALAPPDATA "Programs\$AppName"
$InstalledExe = Join-Path $InstallDir 'aircard.exe'
$DesktopLnk  = Join-Path ([Environment]::GetFolderPath('Desktop')) "$AppName.lnk"
$StartMenuDir = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'
$StartLnk    = Join-Path $StartMenuDir "$AppName.lnk"

function Write-Step($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }
function Write-Ok($msg)   { Write-Host "    $msg" -ForegroundColor Green }
function Write-Warn2($msg) { Write-Host "    $msg" -ForegroundColor Yellow }

function Remove-Shortcuts {
    foreach ($lnk in @($DesktopLnk, $StartLnk)) {
        if (Test-Path $lnk) { Remove-Item $lnk -Force; Write-Ok "Removed shortcut: $lnk" }
    }
}

if ($Uninstall) {
    Write-Step "Uninstalling $AppName"
    Remove-Shortcuts
    if (Test-Path $InstallDir) {
        Remove-Item $InstallDir -Recurse -Force
        Write-Ok "Removed $InstallDir"
    } else {
        Write-Warn2 "Nothing installed at $InstallDir"
    }
    Write-Host "Done." -ForegroundColor Green
    return
}

# 1. Resolve the executable (prefer an existing release build, else build it).
$sourceExe = Join-Path $RepoRoot 'target\release\aircard.exe'
if (-not (Test-Path $sourceExe)) {
    Write-Step "No prebuilt binary found - building from source"
    $cargo = Get-Command cargo -ErrorAction SilentlyContinue
    if (-not $cargo) {
        Write-Warn2 "cargo (Rust) is not installed and no prebuilt aircard.exe was found."
        Write-Warn2 "Install Rust from https://rustup.rs or download aircard.exe from the repo Releases,"
        Write-Warn2 "place it in target\release\, then re-run this script."
        throw "Cannot produce aircard.exe."
    }
    Push-Location $RepoRoot
    try { & cargo build --release; if ($LASTEXITCODE -ne 0) { throw "cargo build failed ($LASTEXITCODE)." } }
    finally { Pop-Location }
}
if (-not (Test-Path $sourceExe)) { throw "Build reported success but $sourceExe is missing." }
Write-Ok "Executable: $sourceExe"

# 2. Install into a stable per-user location.
Write-Step "Installing to $InstallDir"
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Copy-Item $sourceExe $InstalledExe -Force
Write-Ok "Copied aircard.exe"

# 3. Shortcuts.
if (-not $NoShortcut) {
    Write-Step "Creating shortcuts"
    $shell = New-Object -ComObject WScript.Shell
    foreach ($target in @($DesktopLnk, $StartLnk)) {
        $lnk = $shell.CreateShortcut($target)
        $lnk.TargetPath       = $InstalledExe
        $lnk.WorkingDirectory = $InstallDir
        $lnk.IconLocation     = $InstalledExe
        $lnk.Description       = 'Apple Wallet card skinner'
        $lnk.Save()
        Write-Ok "Shortcut: $target"
    }
}

# 4. Apple Mobile Device Support DLL prerequisite check.
Write-Step "Checking Apple Mobile Device Support DLLs"
$dllDirs = @(
    'C:\Program Files\Common Files\Apple\Mobile Device Support',
    'C:\Program Files (x86)\Common Files\Apple\Mobile Device Support',
    $InstallDir,
    (Join-Path $env:LOCALAPPDATA 'AirCard\MobileDeviceSupport')
)
$core = @('CoreFoundation.dll', 'MobileDevice.dll', 'AirTrafficHost.dll')
$found = $false
foreach ($d in $dllDirs) {
    if (Test-Path $d) {
        $have = $true
        foreach ($f in $core) { if (-not (Test-Path (Join-Path $d $f))) { $have = $false; break } }
        if ($have) { $found = $true; Write-Ok "DLLs found in: $d"; break }
    }
}
if (-not $found) {
    Write-Warn2 "Apple Mobile Device Support DLLs were not found."
    Write-Warn2 "AirCard needs the DESKTOP 64-bit iTunes from apple.com (not the Microsoft Store build)."
    Write-Warn2 "The Store 'iTunes' / 'Apple Devices' keeps its DLLs in a sandbox AirCard cannot load."
    $answer = Read-Host "Open the iTunes 64-bit download page now? [y/N]"
    if ($answer -match '^(y|o)') { Start-Process 'https://www.apple.com/itunes/download/win64' }
}

Write-Host ""
Write-Host "$AppName installed." -ForegroundColor Green
Write-Host "  Executable : $InstalledExe"
if (-not $NoShortcut) { Write-Host "  Shortcuts  : Desktop + Start Menu" }

# 5. Launch.
if (-not $NoLaunch) {
    $answer = Read-Host "Launch AirCard now? [Y/n]"
    if ($answer -notmatch '^(n)') { Start-Process $InstalledExe }
}
