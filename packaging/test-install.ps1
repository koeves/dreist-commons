# Check packaging/install.ps1 with a fake app ("Fake"): install, reinstall
# over an older version, a refused incomplete build and uninstall, for this
# user (--user) and for all users (the default; when run as administrator). It installs for real, so it runs on CI runners.
#
#   pwsh packaging/test-install.ps1
$ErrorActionPreference = 'Stop'

$script = Join-Path $PSScriptRoot 'install.ps1'
$work = Join-Path ([IO.Path]::GetTempPath()) ("install-test-" + [Guid]::NewGuid())
$app = 'Fake'
$script:fails = 0

function Ok($what) { "ok   $what" }
function Bad($what) { "FAIL $what"; $script:fails++ }
function Has($what, $path) { if (Test-Path -LiteralPath $path) { Ok $what } else { Bad "$what ($path missing)" } }
function Gone($what, $path) { if (Test-Path -LiteralPath $path) { Bad "$what ($path left)" } else { Ok $what } }
function Binary($path) {
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $path) | Out-Null
    [IO.File]::WriteAllBytes($path, [byte[]](0x4D, 0x5A, 0x90, 0x00))
}
function Run($dir, [string[]]$opts = @()) {
    $out = & pwsh -NoProfile -File (Join-Path $dir 'install.ps1') @opts 2>&1
    @{ Code = $LASTEXITCODE; Out = ($out -join "`n") }
}

try {
    $dist = Join-Path $work 'dist'
    New-Item -ItemType Directory -Force -Path $dist | Out-Null
    Copy-Item $script (Join-Path $dist 'install.ps1')
    Binary (Join-Path $dist "$app.vst3\Contents\x86_64-win\$app.vst3")
    Binary (Join-Path $dist "$app-CLAP\$app.clap")
    Binary (Join-Path $dist "$app-App\$app.exe")

    $vst3 = Join-Path $env:LOCALAPPDATA "Programs\Common\VST3\$app.vst3"
    $clap = Join-Path $env:LOCALAPPDATA "Programs\Common\CLAP\$app\$app.clap"
    $appDir = Join-Path $env:LOCALAPPDATA "Programs\$app"
    $link = Join-Path ([Environment]::GetFolderPath('Programs')) "$app.lnk"

    '== install'
    $r = Run $dist @('--user')
    if ($r.Code -ne 0) { Bad "install exits 0: $($r.Out)" }
    Has 'VST3' $vst3
    Has 'CLAP' $clap
    Has 'app' (Join-Path $appDir "$app.exe")
    Has 'Start menu entry' $link
    if ($r.Out -match '(?m)^VST3: ' -and $r.Out -match 'installed') { Ok 'messages' } else { Bad "messages: $($r.Out)" }

    '== reinstall drops files of an older version'
    $stale = Join-Path $vst3 'Contents\old-file'
    New-Item -ItemType File -Path $stale | Out-Null
    $r = Run $dist @('--user')
    if ($r.Code -ne 0) { Bad 'reinstall exits 0' }
    Gone 'old file' $stale
    Gone 'no .new left' "$vst3.new"

    '== an incomplete build changes nothing'
    $broken = Join-Path $work 'broken'
    Copy-Item $dist $broken -Recurse
    [IO.File]::WriteAllBytes((Join-Path $broken "$app-App\$app.exe"), [byte[]]@())
    $marker = Join-Path $vst3 'marker'
    New-Item -ItemType File -Path $marker | Out-Null
    $r = Run $broken @('--user')
    if ($r.Code -ne 0) { Ok 'refuses an empty binary' } else { Bad 'refuses an empty binary' }
    if ($r.Out -match 'Not a complete build') { Ok 'says why' } else { Bad "error: $($r.Out)" }
    Has 'install untouched' $marker

    '== wrong options'
    $r = Run $dist @('--bogus')
    if ($r.Code -ne 0) { Ok 'unknown option fails' } else { Bad 'unknown option fails' }
    $r = Run $dist @('--help')
    if ($r.Out -match '--uninstall') { Ok 'help lists the options' } else { Bad 'help' }

    '== uninstall'
    $r = Run $dist @('--user', '--uninstall')
    if ($r.Code -ne 0) { Bad 'uninstall exits 0' }
    Gone 'VST3' $vst3
    Gone 'CLAP' $clap
    Gone 'app' $appDir
    Gone 'Start menu entry' $link
    if ($r.Out -match 'removed') { Ok 'message' } else { Bad "message: $($r.Out)" }

    '== uninstall --purge also removes the logs'
    Run $dist @('--user') | Out-Null
    $logs = Join-Path $env:LOCALAPPDATA "$app\Logs"
    New-Item -ItemType Directory -Force -Path $logs | Out-Null
    New-Item -ItemType File -Force -Path (Join-Path $logs 'fake.log') | Out-Null
    $r = Run $dist @('--user', '--uninstall')
    Has 'logs kept without --purge' (Join-Path $logs 'fake.log')
    if ($r.Out -match 'logs in') { Ok 'says where the logs stay' } else { Bad "message: $($r.Out)" }
    Run $dist @('--user') | Out-Null
    $r = Run $dist @('--user', '--uninstall', '--purge')
    if ($r.Code -ne 0) { Bad '--purge exits 0' }
    Gone 'logs' $logs
    Gone 'VST3' $vst3
    if ($r.Out -match 'with its logs') { Ok 'message' } else { Bad "message: $($r.Out)" }
    $parent = Join-Path $env:LOCALAPPDATA $app
    if ((Test-Path -LiteralPath $parent) -and -not (Get-ChildItem -LiteralPath $parent)) { Remove-Item -LiteralPath $parent }

    '== uninstall by name, with nothing next to the script'
    Run $dist @('--user') | Out-Null
    $bare = Join-Path $work 'bare'
    New-Item -ItemType Directory -Force -Path $bare | Out-Null
    Copy-Item $script (Join-Path $bare 'install.ps1')
    $r = Run $bare @('--user', '--app', $app, '--uninstall')
    if ($r.Code -ne 0) { Bad '--app uninstall exits 0' }
    Gone 'VST3' $vst3

    $admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
        [Security.Principal.WindowsBuiltInRole]::Administrator)
    if ($admin) {
        '== all users (default, and --system)'
        foreach ($opts in @(@(), @('--system'))) {
            $r = Run $dist $opts
            if ($r.Code -ne 0) { Bad "install $opts exits 0: $($r.Out)" }
            Has 'VST3' (Join-Path $env:CommonProgramFiles "VST3\$app.vst3")
            Has 'CLAP' (Join-Path $env:CommonProgramFiles "CLAP\$app\$app.clap")
            Has 'app' (Join-Path $env:ProgramFiles "$app\$app.exe")
            Has 'Start menu entry' (Join-Path ([Environment]::GetFolderPath('CommonPrograms')) "$app.lnk")
        }
        Gone 'nothing for this user' $vst3
        $r = Run $dist @('--uninstall')
        if ($r.Code -ne 0) { Bad 'uninstall exits 0' }
        Gone 'VST3' (Join-Path $env:CommonProgramFiles "VST3\$app.vst3")
        Gone 'app' (Join-Path $env:ProgramFiles $app)
    } else {
        '== all users: skipped (not administrator)'
    }
} finally {
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
}

if ($script:fails -eq 0) { 'all passed' } else { "$($script:fails) failed"; exit 1 }
