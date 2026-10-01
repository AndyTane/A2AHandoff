# The files a user's agent reads: AGENTS.md and the installable skill.
#
# These fail silently when they are wrong. A skill whose front matter does not parse simply
# never appears in an agent's catalog, and a description that does not name the situations the
# user is in never matches anything - neither shows up as an error anywhere. So the shape is
# asserted here, in the same style as the other contract tests.
#
# Nothing is started, nothing is installed, and no message is sent.
[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

$root = Split-Path $PSScriptRoot -Parent
$failures = [Collections.Generic.List[string]]::new()
$checks = 0

function Check($name, $condition, $detail = '') {
    $script:checks++
    if ($condition) {
        Write-Output "  ok   $name"
    } else {
        Write-Output "  FAIL $name $detail"
        $script:failures.Add($name)
    }
}

function Read-Text($path) {
    return [IO.File]::ReadAllText($path, [Text.Encoding]::UTF8)
}

Write-Output 'AGENTS.md'
$agentsPath = Join-Path $root 'AGENTS.md'
Check 'AGENTS.md exists at the product root' (Test-Path $agentsPath) $agentsPath
$agents = Read-Text $agentsPath
# It has to be the procedure, not a pointer to one.
Check 'it carries the configure command' ($agents -match 'scripts\\bootstrap\.ps1') ''
Check 'it carries the start command' ($agents -match 'A2AHandoff\.exe') ''
Check 'it names the binding step' ($agents -match '绑定配置') ''
Check 'it states what a fresh install looks like' ($agents -match '未绑定') ''
Check 'it draws the no-sending boundary' `
    ($agents -match '(?s)must never do.*发送给') 'the send buttons are the user''s decision'
Check 'it says the binding needs the user' ($agents -match '(?s)needs the user''s decision') ''

Write-Output 'Skills'
$skillsDir = Join-Path $root 'skills'
Check 'skills/ exists' (Test-Path $skillsDir) $skillsDir
$skillDirs = @(Get-ChildItem $skillsDir -Directory -ErrorAction SilentlyContinue)
Check 'at least one skill is shipped' ($skillDirs.Count -ge 1) "found $($skillDirs.Count)"
foreach ($dir in $skillDirs) {
    $file = Join-Path $dir.FullName 'SKILL.md'
    Check "$($dir.Name): has SKILL.md" (Test-Path $file) $file
    if (-not (Test-Path $file)) { continue }
    $text = Read-Text $file
    # The catalog reads `name` and `description` from the front matter; a file that does not
    # start with the fence is treated as having none at all.
    Check "$($dir.Name): starts with the front matter fence" ($text.StartsWith("---`n")) 'first line must be ---'
    $nameMatch = [regex]::Match($text, '(?m)^name:\s*(\S+)\s*$')
    Check "$($dir.Name): declares a name" $nameMatch.Success 'no name: line in the front matter'
    if ($nameMatch.Success) {
        Check "$($dir.Name): the name matches its directory" `
            ($nameMatch.Groups[1].Value -ceq $dir.Name) "name is '$($nameMatch.Groups[1].Value)'"
    }
    $descMatch = [regex]::Match($text, '(?m)^description:\s*(.+)$')
    Check "$($dir.Name): declares a one-line description" $descMatch.Success 'no description: line'
    if ($descMatch.Success) {
        $desc = $descMatch.Groups[1].Value.Trim()
        # This string is the whole trigger surface: it is what an agent matches a user's
        # situation against, so it has to be specific and it has to be long enough to say
        # anything at all.
        Check "$($dir.Name): the description is specific enough" ($desc.Length -ge 80) "only $($desc.Length) chars"
        Check "$($dir.Name): the description names the tool" ($desc -match 'A2AHandoff') $desc
    }
    # A placeholder that only says "use when relevant" would match nothing useful.
    Check "$($dir.Name): the body is not a stub" ($text.Length -ge 1500) "$($text.Length) chars"
}

Write-Output 'The release archive carries both'
$packager = Read-Text (Join-Path $root 'scripts\package-release.ps1')
Check 'package-release.ps1 copies AGENTS.md' ($packager -match "AGENTS\.md") ''
Check 'package-release.ps1 copies skills/' ($packager -match "Copy-Item.*'skills'") ''
$readme = Read-Text (Join-Path $root 'README.md')
Check 'README points at AGENTS.md' ($readme -match 'AGENTS\.md') ''
Check 'README points at the skill' ($readme -match 'skills/a2a-handoff-setup') ''

Write-Output ''
Write-Output "Agent docs: $checks check(s), $($failures.Count) failure(s)."
if ($failures.Count) {
    $failures | ForEach-Object { Write-Error "failed: $_" }
    exit 1
}
Write-Output 'The agent-facing docs are well formed. Nothing was installed and no message was sent.'
