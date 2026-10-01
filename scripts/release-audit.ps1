[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

$root = Split-Path $PSScriptRoot -Parent
$tracked = @(git -C $root ls-files)
if ($LASTEXITCODE -ne 0) { throw 'Not a Git repository.' }

$forbidden = @($tracked | Where-Object {
    $_ -match '^(backups|artifacts|target|\.ui-staging)/' -or
    ($_ -match '^runtime/' -and $_ -ne 'runtime/.gitkeep') -or
    $_ -eq 'crates/handoff-windows-adapter/Cargo.toml' -or
    $_ -match '^crates/handoff-windows-adapter/' -or
    $_ -in @('DSH_TASKS.md','design_reference.html','docs/CLAUDE_REPLY_READER_FIX.md','docs/DSH_INPUT_FIX.md')
})
if ($forbidden.Count) {
    $forbidden | ForEach-Object { Write-Error "Forbidden tracked path: $_" }
    exit 2
}

function Git-Grep($Pattern) {
    $out = @(git -C $root grep --cached -n -E $Pattern -- . 2>$null)
    if ($LASTEXITCODE -gt 1) { throw "git grep failed for: $Pattern" }
    return $out
}
$private = @()
$private += Git-Grep '[A-Za-z]:\\Users\\[^\\]+\\'
$private += Git-Grep '[A-Za-z]:\\(Tools|Projects|01_Projects)\\'
if ($private.Count) {
    $private | ForEach-Object { Write-Error "Private machine data: $_" }
    exit 3
}

$realSessions = @()
$realSessions += Git-Grep 'cse_[A-Za-z0-9_-]{16,}'
$realSessions += Git-Grep 'session-[0-9a-fA-F]{8}-[0-9a-fA-F-]{20,}'
if ($realSessions.Count) {
    $realSessions | ForEach-Object { Write-Error "Possible real session identifier: $_" }
    exit 4
}

$secrets = @(Git-Grep '(token|password|secret|api[_-]?key)[[:space:]]*[:=][[:space:]]*[^[:space:]]+')
if ($secrets.Count) {
    $secrets | ForEach-Object { Write-Error "Possible secret assignment: $_" }
    exit 5
}

git -C $root diff --cached --check
if ($LASTEXITCODE -ne 0) { exit 6 }

Write-Output "Release audit passed: $($tracked.Count) tracked/staged files checked."
Write-Output 'No runtime state, obvious private paths, real-looking session IDs, or secret assignments were found.'
