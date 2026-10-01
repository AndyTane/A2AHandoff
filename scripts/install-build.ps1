# Installs the freshly built binaries into target\release.
#
# The running A2AHandoff holds its .exe open, so Windows refuses to overwrite it
# while the app is up. This script checks first and tells you what to do instead of
# half-copying.
[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

$root = Split-Path $PSScriptRoot -Parent
$dist = Join-Path $root 'dist'
$target = Join-Path $root 'target\release'

if (-not (Test-Path $dist)) { throw "No dist\ directory. Build with: `$env:CARGO_TARGET_DIR='<somewhere>'; cargo build --release" }

$running = @(Get-Process A2AHandoff, handoff-runtime -ErrorAction SilentlyContinue)
if ($running.Count) {
    Write-Output 'A2AHandoff is still running, so its .exe cannot be replaced:'
    $running | ForEach-Object { Write-Output "  $($_.ProcessName) (PID $($_.Id))" }
    Write-Output ''
    Write-Output 'Close A2AHandoff, then run this script again.'
    Write-Output 'Your session state (runtime\workflow.json, receipts, bindings) is untouched by a restart.'
    exit 2
}

foreach ($name in @('A2AHandoff.exe', 'handoff-runtime.exe')) {
    $from = Join-Path $dist $name
    $to = Join-Path $target $name
    if (-not (Test-Path $from)) { throw "missing $from" }
    Copy-Item $from $to -Force
    $info = Get-Item $to
    Write-Output ("installed {0}  {1} bytes  {2}" -f $info.Name, $info.Length, $info.LastWriteTime)
}
Write-Output ''
Write-Output 'Done. Start A2AHandoff with: .\target\release\A2AHandoff.exe --product-root .'
