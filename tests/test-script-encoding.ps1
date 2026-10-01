# Encoding guard for PowerShell sources.
#
# Windows PowerShell 5.1 reads a .ps1 file using the system ANSI code page unless
# the file starts with a UTF-8 BOM. A script that stores UTF-8 Chinese (or any
# non-ASCII) text without a BOM therefore fails to PARSE on Chinese-locale Windows,
# and the adapter that dot-sources it dies with no output at all — which surfaces
# far away as "adapter_output_invalid:" with an empty detail.
#
# That is exactly what happened when `adapters/windows/composer-draft.ps1` lost its
# BOM during an edit: the DSH -> Claude draft step stopped working entirely.
#
# This test fails if any PowerShell file in the repository contains non-ASCII bytes
# without a UTF-8 BOM, and it parses every script the way 5.1 actually would.
[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

$root = Split-Path $PSScriptRoot -Parent
$skip = '\\(target|backups|artifacts|\.git|\.ui-staging|node_modules)\\'
$files = @(Get-ChildItem $root -Recurse -File -Filter *.ps1 -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -notmatch $skip })

$missingBom = [Collections.Generic.List[string]]::new()
$parseErrors = [Collections.Generic.List[string]]::new()
$checked = 0
$nonAscii = 0

foreach ($f in $files) {
    $checked++
    $bytes = [IO.File]::ReadAllBytes($f.FullName)
    if ($bytes.Length -lt 3) { continue }
    $hasBom = ($bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF)
    $hasNonAscii = $false
    foreach ($b in $bytes) { if ($b -gt 0x7F) { $hasNonAscii = $true; break } }
    $relative = $f.FullName.Substring($root.Length + 1)
    if ($hasNonAscii) {
        $nonAscii++
        if (-not $hasBom) { $missingBom.Add($relative) }
    }
}

# Parse every script the way Windows PowerShell 5.1 resolves the file on disk
# (i.e. honouring the BOM / ANSI default), not by pre-decoding as UTF-8.
$parseScript = {
    param($path)
    $errors = $null
    [void][System.Management.Automation.Language.Parser]::ParseFile($path, [ref]$null, [ref]$errors)
    if ($errors -and $errors.Count) { return $errors[0].Message }
    return ''
}
foreach ($f in $files) {
    $msg = & $parseScript $f.FullName
    if ($msg) { $parseErrors.Add("$($f.FullName.Substring($root.Length + 1)): $msg") }
}

Write-Output "Checked $checked PowerShell file(s); $nonAscii contain non-ASCII text."

if ($missingBom.Count) {
    Write-Output ''
    Write-Output 'Non-ASCII without a UTF-8 BOM (breaks parsing under Windows PowerShell 5.1):'
    $missingBom | ForEach-Object { Write-Output "  $_" }
}
if ($parseErrors.Count) {
    Write-Output ''
    Write-Output 'Scripts that fail to parse:'
    $parseErrors | ForEach-Object { Write-Output "  $($_.Substring(0, [Math]::Min(200, $_.Length)))" }
}

if ($missingBom.Count -or $parseErrors.Count) {
    Write-Error "PowerShell encoding/parse check failed ($($missingBom.Count) missing BOM, $($parseErrors.Count) parse error(s))."
    exit 1
}
Write-Output 'All PowerShell sources parse correctly and every non-ASCII file carries a UTF-8 BOM. No messages were sent.'
