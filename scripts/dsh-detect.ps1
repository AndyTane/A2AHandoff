# Detection helpers for scripts/bootstrap.ps1 and the first-run tests.
#
# They live in their own file so a test can exercise them without launching a process or
# writing a config - the inline version this replaces was only ever reached on a machine
# with a running DSH, so nothing in the suite covered it.
Set-StrictMode -Version Latest

# The install root of the DSH process described by a node.exe command line, or $null when the
# line is not one.
#
# The process carries its paths on its own command line, and the node.exe running it is
# normally an absolute path too:
#
#   "C:\Program Files\nodejs\node.exe" D:\apps\dsh\runtime\node_modules\@deepseek-ai\dsh\lib\bin.js web
#
# Scanning that for the first drive-lettered path and stretching it to the marker captures
# node's own path, the closing quote between the two arguments and everything up to the
# marker:
#
#   C:\Program Files\nodejs\node.exe" D:\apps\dsh
#
# That is not a path, and `Test-Path` rejects it with "Illegal characters in path" - which is
# how the first-run step failed on a machine whose DSH is launched by an absolute node path.
# Split the line into arguments the way Windows does, and test each one on its own.
function Get-DshInstallRoot([string]$CommandLine) {
    if ([string]::IsNullOrWhiteSpace($CommandLine)) { return $null }
    $segments = $CommandLine -split '"'
    for ($i = 0; $i -lt $segments.Count; $i++) {
        # Odd segments sit between quotes, so each is exactly one argument and may contain
        # spaces. The even segments are the unquoted runs, which Windows splits on whitespace.
        $tokens = if (($i % 2) -eq 1) { @($segments[$i]) } else { @($segments[$i] -split '\s+') }
        foreach ($token in $tokens) {
            if ($token -match '(?i)^([A-Za-z]:\\.*)\\runtime\\node_modules\\@deepseek-ai\\dsh\\lib\\bin\.js$') {
                return $Matches[1]
            }
        }
    }
    return $null
}

# The same rule for a whole process list, so the caller does not repeat the loop.
function Get-DshDataHomeFromCommandLines([string[]]$CommandLines) {
    foreach ($line in @($CommandLines)) {
        $installRoot = Get-DshInstallRoot $line
        if (-not $installRoot) { continue }
        $candidate = Join-Path $installRoot 'data'
        if (Test-Path (Join-Path $candidate 'storages\session_projcache\sessions')) {
            return $candidate
        }
    }
    return $null
}
