# Install the app next to this script (standalone app, VST3, CLAP) on
# Windows, or remove it. packaging/install.sh does the same on Linux and macOS.
#
#   .\install.ps1                         install for all users (administrator PowerShell)
#   .\install.ps1 --user                  install for this user only
#   .\install.ps1 --uninstall             remove a system-wide install
#   .\install.ps1 --uninstall --user      remove an install for this user
#   .\install.ps1 --uninstall --purge     also remove this user's diagnostics logs
#   .\install.ps1 --app <Name> --uninstall    remove without the files at hand
#   (--system, the default here, is accepted as on Linux and macOS)
#
# Next to this script, each part installed when present: <App>.vst3,
# <App>-CLAP\ (<App>.clap + Resources\), <App>-App\ (<App>.exe + Resources\).
#
# The install locations are listed in packaging/README.md; settings and
# content are left alone.
$ErrorActionPreference = 'Stop'

$here = $PSScriptRoot
$scope = 'system'
$action = 'install'
$purge = $false
$app = $null
$argv = @($args)

function Show-Usage {
    $lines = Get-Content -LiteralPath $PSCommandPath
    foreach ($l in $lines[1..($lines.Count - 1)]) {
        if ($l -notmatch '^#') { break }
        $l -replace '^# ?', ''
    }
}

for ($i = 0; $i -lt $argv.Count; $i++) {
    switch -Exact ($argv[$i]) {
        '--uninstall' { $action = 'uninstall' }
        '--purge' { $purge = $true }
        '--system' { $scope = 'system' }
        '--user' { $scope = 'user' }
        '--app' {
            if ($i + 1 -ge $argv.Count) { Show-Usage | Write-Host; exit 2 }
            $i++
            $app = $argv[$i]
        }
        { $_ -in '-h', '--help' } { Show-Usage; exit 0 }
        default { Show-Usage | Write-Host; exit 2 }
    }
}

# The app's name comes from the parts next to this script.
if (-not $app) {
    foreach ($p in Get-ChildItem -LiteralPath $here -Directory) {
        $n = $p.Name -replace '(\.vst3|-CLAP|-App)$', ''
        if ($n -eq $p.Name) { continue }
        if (-not $app) { $app = $n }
        elseif ($app -ne $n) {
            [Console]::Error.WriteLine("More than one app next to this script ($app, $n), nothing changed")
            exit 2
        }
    }
}
if (-not $app) {
    [Console]::Error.WriteLine("Nothing to install next to this script ($here); --app <Name> names the app to remove")
    exit 2
}

$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
if ($scope -eq 'system' -and -not $admin) {
    [Console]::Error.WriteLine('Installing for all users needs an administrator PowerShell; --user installs for this user only')
    exit 2
}

# Install locations (packaging/README.md lists the same).
if ($scope -eq 'user') {
    $common = Join-Path $env:LOCALAPPDATA 'Programs\Common'
    $vst3Dir = Join-Path $common 'VST3'
    $clapDir = Join-Path $common "CLAP\$app"
    $appDir = Join-Path $env:LOCALAPPDATA "Programs\$app"
    $menuDir = [Environment]::GetFolderPath('Programs')
} else {
    $vst3Dir = Join-Path $env:CommonProgramFiles 'VST3'
    $clapDir = Join-Path $env:CommonProgramFiles "CLAP\$app"
    $appDir = Join-Path $env:ProgramFiles $app
    $menuDir = [Environment]::GetFolderPath('CommonPrograms')
}
$settings = if ($scope -eq 'user') { Join-Path $env:APPDATA $app } else { "%APPDATA%\$app" }
$shortcut = Join-Path $menuDir "$app.lnk"

function Remove-Part($path) {
    if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Recurse -Force }
}

function Remove-Install {
    Remove-Part (Join-Path $vst3Dir "$app.vst3")
    Remove-Part $clapDir
    Remove-Part $appDir
    Remove-Part $shortcut
}

# The diagnostics logs (dreist_engine::diag), this user's.
$logs = Join-Path $env:LOCALAPPDATA "$app\Logs"

if ($action -eq 'uninstall') {
    Remove-Install
    if ($purge) {
        Remove-Part $logs
        "$app removed with its logs (settings in $settings and content kept)"
    } else {
        "$app removed (settings in $settings, logs in $logs and content kept)"
    }
    exit 0
}

# The parts next to this script, each checked to be a complete build before
# anything installed is touched.
$vst3 = Join-Path $here "$app.vst3"
$clap = Join-Path $here "$app-CLAP"
$appSrc = Join-Path $here "$app-App"

function Assert-Build($file) {
    $ok = $false
    if (Test-Path -LiteralPath $file -PathType Leaf) {
        $stream = [IO.File]::OpenRead($file)
        try {
            $head = New-Object byte[] 2
            $ok = ($stream.Read($head, 0, 2) -eq 2) -and $head[0] -eq 0x4D -and $head[1] -eq 0x5A
        } finally { $stream.Dispose() }
    }
    if (-not $ok) {
        [Console]::Error.WriteLine("Not a complete build, nothing changed: $file")
        exit 1
    }
}

$found = $false
if (Test-Path -LiteralPath $vst3) {
    $bin = Get-ChildItem -LiteralPath (Join-Path $vst3 'Contents') -Directory -Filter '*-win' -ErrorAction SilentlyContinue |
        ForEach-Object { Join-Path $_.FullName "$app.vst3" } | Select-Object -First 1
    if (-not $bin) { $bin = Join-Path $vst3 "Contents\x86_64-win\$app.vst3" }
    Assert-Build $bin
    $found = $true
}
if (Test-Path -LiteralPath $clap) { Assert-Build (Join-Path $clap "$app.clap"); $found = $true }
if (Test-Path -LiteralPath $appSrc) { Assert-Build (Join-Path $appSrc "$app.exe"); $found = $true }
if (-not $found) {
    [Console]::Error.WriteLine("Nothing to install for Windows next to this script ($here)")
    exit 2
}

# Copy next to the destination, then swap it in, so an interrupted copy
# leaves no half install.
function Set-Part($from, $to) {
    $parent = Split-Path -Parent $to
    New-Item -ItemType Directory -Force -Path $parent | Out-Null
    Remove-Part "$to.new"
    Copy-Item -LiteralPath $from -Destination "$to.new" -Recurse
    Remove-Part $to
    Move-Item -LiteralPath "$to.new" -Destination $to
}

# A reinstall replaces everything, so files of an older version do not stay.
Remove-Install
if (Test-Path -LiteralPath $vst3) {
    Set-Part $vst3 (Join-Path $vst3Dir "$app.vst3")
    "VST3: $(Join-Path $vst3Dir "$app.vst3")"
}
if (Test-Path -LiteralPath $clap) {
    Set-Part $clap $clapDir
    "CLAP: $clapDir"
}
if (Test-Path -LiteralPath $appSrc) {
    Set-Part $appSrc $appDir
    $exe = Join-Path $appDir "$app.exe"
    New-Item -ItemType Directory -Force -Path $menuDir | Out-Null
    $link = (New-Object -ComObject WScript.Shell).CreateShortcut($shortcut)
    $link.TargetPath = $exe
    $link.WorkingDirectory = $appDir
    $link.Save()
    "App: $appDir (Start menu `"$app`")"
}
"$app installed (settings in $settings)"
