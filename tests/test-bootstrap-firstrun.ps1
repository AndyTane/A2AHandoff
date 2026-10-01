# First-run / bootstrap tests.
#
# scripts/bootstrap.ps1 resolves the product root from its own location, so this
# test runs it inside a throwaway COPY of the product and never touches the real
# runtime/ directory. Nothing is started and no message is sent.
[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

$root = Split-Path $PSScriptRoot -Parent
. (Join-Path $root 'scripts\dsh-detect.ps1')
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

# Run bootstrap in a child process and capture BOTH streams. Several cases are
# expected to fail, so stderr must not become a terminating error here.
#
# The argument list is built explicitly: `powershell -File` receives every value as
# a STRING, so a splatted `Force = $true` would bind as "True" and fail to convert
# to a switch. Switches must be passed bare.
function Invoke-Bootstrap($arguments) {
    $childArgs = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $bootstrap)
    foreach ($key in $arguments.Keys) {
        $value = $arguments[$key]
        if ($value -is [switch] -or $value -is [bool]) {
            if ($value) { $childArgs += "-$key" }
        } elseif ($value -is [array]) {
            $childArgs += "-$key"
            $childArgs += ($value | ForEach-Object { [string]$_ })
        } else {
            $childArgs += "-$key"
            $childArgs += [string]$value
        }
    }
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $all = & powershell @childArgs 2>&1
        return ($all | Out-String)
    } catch {
        return ($_ | Out-String)
    } finally {
        $ErrorActionPreference = $prev
    }
}

# Run bootstrap IN THIS process so `$PSBoundParameters` is genuinely populated,
# which is what the "explicit argument beats detection" behaviour depends on.
# The child-process helper above cannot exercise that path.
function Invoke-BootstrapInProcess($arguments) {
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $all = & $bootstrap @arguments *>&1
        return ($all | Out-String)
    } catch {
        return ($_ | Out-String)
    } finally {
        $ErrorActionPreference = $prev
    }
}

# A copy of just what bootstrap needs: the script, the examples it copies from,
# and an empty runtime directory.
$stage = Join-Path ([IO.Path]::GetTempPath()) ("a2a-bootstrap-" + [Guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Force (Join-Path $stage 'scripts') | Out-Null
New-Item -ItemType Directory -Force (Join-Path $stage 'examples') | Out-Null
Copy-Item (Join-Path $root 'scripts\bootstrap.ps1') (Join-Path $stage 'scripts') -Force
Copy-Item (Join-Path $root 'scripts\dsh-detect.ps1') (Join-Path $stage 'scripts') -Force
Copy-Item (Join-Path $root 'examples\message-templates.json') (Join-Path $stage 'examples') -Force
Copy-Item (Join-Path $root 'examples\config.json') (Join-Path $stage 'examples') -Force
$bootstrap = Join-Path $stage 'scripts\bootstrap.ps1'
$configPath = Join-Path $stage 'runtime\config.json'
$bindingsPath = Join-Path $stage 'runtime\bindings.json'
$templatesPath = Join-Path $stage 'runtime\message-templates.json'

# A DSH-shaped data directory so detection/validation passes.
$dataHome = Join-Path $stage 'dsh-data'
New-Item -ItemType Directory -Force (Join-Path $dataHome 'storages\session_projcache\sessions') | Out-Null
New-Item -ItemType Directory -Force (Join-Path $dataHome 'sessions') | Out-Null

try {
    Write-Output 'Fresh bootstrap'

    $out = Invoke-BootstrapInProcess @{ DshDataHome = $dataHome; DshWebOrigin = 'http://127.0.0.1:45999' }
    $summary = $null
    try { $summary = $out.Trim() | ConvertFrom-Json } catch { }
    Check 'bootstrap succeeded and printed a summary' ($null -ne $summary -and $summary.ok -eq $true) `
        "output: $($out | Select-Object -First 3)"

    Check 'runtime/config.json created' (Test-Path $configPath)
    Check 'runtime/bindings.json created' (Test-Path $bindingsPath)
    Check 'runtime/message-templates.json created' (Test-Path $templatesPath)

    $cfg = [IO.File]::ReadAllText($configPath, [Text.Encoding]::UTF8) | ConvertFrom-Json
    $names = @($cfg.PSObject.Properties.Name)
    Check 'config has exactly the 9 schema fields' ($names.Count -eq 9) "got $($names.Count): $($names -join ',')"
    foreach ($f in @('enabled','poll_seconds','dispatch_delay_seconds','dsh_data_home',
                     'dsh_web_origin','dsh_browser_processes','workspace',
                     'claude_host','dsh_page_title_pattern')) {
        Check "config field present: $f" ($names -contains $f)
    }
    Check 'enabled defaults to false' ($cfg.enabled -eq $false)
    Check 'poll_seconds defaults to 60' ($cfg.poll_seconds -eq 60)
    Check 'dispatch_delay_seconds defaults to 10' ($cfg.dispatch_delay_seconds -eq 10)
    Check 'workspace defaults to empty' ($cfg.workspace -eq '')
    Check 'claude_host defaults to claude.ai' ($cfg.claude_host -eq 'claude.ai')
    Check 'dsh_page_title_pattern defaults to DeepSeek Harness' ($cfg.dsh_page_title_pattern -eq 'DeepSeek Harness')
    Check 'dsh_browser_processes defaults to msedge,chrome' `
        ((@($cfg.dsh_browser_processes) -join ',') -eq 'msedge,chrome')
    Check 'dsh_web_origin normalised to scheme://host:port' ($cfg.dsh_web_origin -eq 'http://127.0.0.1:45999')
    Check 'dsh_data_home recorded' `
        ([IO.Path]::GetFullPath($cfg.dsh_data_home) -eq [IO.Path]::GetFullPath($dataHome))

    $bind = [IO.File]::ReadAllText($bindingsPath, [Text.Encoding]::UTF8) | ConvertFrom-Json
    Check 'placeholder claude_session' ($bind.claude_session -eq 'cse_unconfigured')
    Check 'placeholder dsh_session' ($bind.dsh_session -eq 'session-unconfigured')
    Check 'placeholder claude_title' ($bind.claude_title -eq 'Claude Desktop')

    $tpl = [IO.File]::ReadAllText($templatesPath, [Text.Encoding]::UTF8) | ConvertFrom-Json
    Check 'message templates copied and parseable' ($null -ne $tpl.to_dsh -and $null -ne $tpl.to_claude)

    Write-Output 'Re-run safety'
    $before = (Get-FileHash $configPath -Algorithm SHA256).Hash
    $err = Invoke-BootstrapInProcess @{ DshDataHome = $dataHome }
    $after = (Get-FileHash $configPath -Algorithm SHA256).Hash
    Check 'second run without -Force refuses to overwrite' `
        (($err | Out-String) -match 'already exists')
    Check 'existing config is byte-identical after the refusal' ($before -eq $after)

    # A user's real bindings must survive a re-run too.
    [IO.File]::WriteAllText($bindingsPath, '{"claude_session":"cse_mine"}', [Text.UTF8Encoding]::new($false))
    $null = Invoke-BootstrapInProcess @{ DshDataHome = $dataHome; Force = $true }
    Check 'bindings are not overwritten by bootstrap' `
        (([IO.File]::ReadAllText($bindingsPath) | ConvertFrom-Json).claude_session -eq 'cse_mine')

    Write-Output 'Validation and normalisation'
    $before = (Get-FileHash $configPath -Algorithm SHA256).Hash
    $err = Invoke-BootstrapInProcess @{ DshDataHome = $dataHome; DshWebOrigin = 'ftp://example.com'; Force = $true }
    $after = (Get-FileHash $configPath -Algorithm SHA256).Hash
    Check 'non-http origin is rejected' (($err | Out-String) -match 'Invalid DSH web origin')
    Check 'rejected origin writes nothing' ($before -eq $after)

    $err = Invoke-BootstrapInProcess @{ DshDataHome = (Join-Path $stage 'not-a-dsh-dir'); Force = $true }
    Check 'a directory without the DSH layout is rejected' `
        (($err | Out-String) -match 'Not a supported DSH data directory')

    # NOTE: an EMPTY array cannot be marshalled to `powershell -File` (the child sees
    # a missing argument). The empty-list fallback is covered by
    # tests/test-adapter-config.ps1 and by the Rust unit tests instead.
    $null = Invoke-BootstrapInProcess @{
        DshDataHome = $dataHome
        DshWebOrigin = 'HTTP://LocalHost:3080/some/path'
        DshBrowserProcesses = @(' MSEDGE ', ' Brave ')
        ClaudeHost = '  CLAUDE.AI  '
        DshPageTitlePattern = ''
        Force = $true
    }
    $cfg = [IO.File]::ReadAllText($configPath, [Text.Encoding]::UTF8) | ConvertFrom-Json
    Check 'origin with path/case normalised' ($cfg.dsh_web_origin -eq 'http://localhost:3080') `
        "got '$($cfg.dsh_web_origin)'"
    Check 'browser list trimmed and lower-cased' `
        ((@($cfg.dsh_browser_processes) -join ',') -eq 'msedge,brave') `
        "got '$(@($cfg.dsh_browser_processes) -join ',')'"
    Check 'claude_host trimmed and lower-cased' ($cfg.claude_host -eq 'claude.ai') `
        "got '$($cfg.claude_host)'"
    Check 'blank title pattern falls back' ($cfg.dsh_page_title_pattern -eq 'DeepSeek Harness') `
        "got '$($cfg.dsh_page_title_pattern)'"

    Write-Output 'Schema parity with Rust and the example'
    $rust = [IO.File]::ReadAllText((Join-Path $root 'crates\handoff-core\src\config.rs'), [Text.Encoding]::UTF8)
    $example = [IO.File]::ReadAllText((Join-Path $root 'examples\config.json'), [Text.Encoding]::UTF8) | ConvertFrom-Json
    $exampleNames = @($example.PSObject.Properties.Name)
    Check 'examples/config.json has exactly the 9 fields' ($exampleNames.Count -eq 9) "got $($exampleNames.Count)"
    Check 'example field set equals bootstrap field set' `
        ((($exampleNames | Sort-Object) -join ',') -eq ((@('enabled','poll_seconds','dispatch_delay_seconds',
            'dsh_data_home','dsh_web_origin','dsh_browser_processes','workspace','claude_host',
            'dsh_page_title_pattern') | Sort-Object) -join ','))
    Check 'no deprecated keys in the example' `
        (-not ($exampleNames -contains 'task_file' -or $exampleNames -contains 'mode' -or $exampleNames -contains 'next_poll_at_ms'))
    Check 'no deprecated keys written by bootstrap' `
        (-not ($names -contains 'task_file' -or $names -contains 'mode' -or $names -contains 'next_poll_at_ms'))
    Check 'Rust declares the deprecated keys it ignores' `
        ($rust -match 'DEPRECATED_KEYS' -and $rust -match '"task_file"' -and $rust -match '"mode"' -and $rust -match '"next_poll_at_ms"')
} finally {
    Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
}

# DSH command-line detection. This is the rule bootstrap uses when it is not given
# -DshDataHome, and it is what a new user's first command runs. It used to be an inline
# pattern that grabbed the FIRST drive-lettered path on the line and stretched it to the
# marker, so a machine that launches DSH with an absolute node path produced
#   C:\Program Files\nodejs\node.exe" D:\apps\dsh
# - node's own path, the quote between the arguments and everything up to the marker - and
# `Test-Path` rejected it as an illegal path. Nothing in the suite reached that branch,
# because every case below passed -DshDataHome explicitly.
#
# These fixtures are repeated in crates/handoff-core/src/config.rs, so the PowerShell rule
# here and the Rust rule in detect_dsh cannot drift apart unnoticed.
Write-Output 'DSH command-line detection'
$quoted = '"C:\Program Files\nodejs\node.exe" D:\apps\dsh\runtime\node_modules\@deepseek-ai\dsh\lib\bin.js web --host 127.0.0.1 --port 3080'
Check 'a quoted node path does not swallow the dsh root' `
    ((Get-DshInstallRoot $quoted) -eq 'D:\apps\dsh') "got '$((Get-DshInstallRoot $quoted))'"
$quotedPlain = '"C:\nodejs\node.exe" D:\apps\dsh\runtime\node_modules\@deepseek-ai\dsh\lib\bin.js web'
Check 'a quote-free node path works too' `
    ((Get-DshInstallRoot $quotedPlain) -eq 'D:\apps\dsh') "got '$((Get-DshInstallRoot $quotedPlain))'"
$entryQuoted = 'node "D:\apps\dsh\runtime\node_modules\@deepseek-ai\dsh\lib\bin.js" web'
Check 'the entry script may be the quoted argument' `
    ((Get-DshInstallRoot $entryQuoted) -eq 'D:\apps\dsh') "got '$((Get-DshInstallRoot $entryQuoted))'"
$bare = 'D:\nodejs\node.exe D:\apps\dsh\runtime\node_modules\@deepseek-ai\dsh\lib\bin.js web'
Check 'neither argument needs quoting' `
    ((Get-DshInstallRoot $bare) -eq 'D:\apps\dsh') "got '$((Get-DshInstallRoot $bare))'"
$runner = '"C:\Program Files\nodejs\node.exe" D:\apps\dsh\runtime\node_modules\@deepseek-ai\dsh-subprocess-local\lib\runner.js -- cmd'
Check 'another dsh process is not the install root' `
    ($null -eq (Get-DshInstallRoot $runner)) "got '$((Get-DshInstallRoot $runner))'"
Check 'an empty command line yields nothing' ($null -eq (Get-DshInstallRoot '')) ''
Check 'a relative entry script is not an install root' `
    ($null -eq (Get-DshInstallRoot 'node runtime\node_modules\@deepseek-ai\dsh\lib\bin.js web')) ''

# A data home is only accepted when it has the layout DSH writes.
$detectStage = Join-Path ([IO.Path]::GetTempPath()) ("a2a-detect-" + [Guid]::NewGuid().ToString('N').Substring(0, 8))
try {
    New-Item -ItemType Directory -Force (Join-Path $detectStage 'dsh-data\storages\session_projcache\sessions') | Out-Null
    $shaped = '"C:\Program Files\nodejs\node.exe" D:\apps\dsh\runtime\node_modules\@deepseek-ai\dsh\lib\bin.js web'
    Check 'a line without a usable data directory yields nothing' `
        ($null -eq (Get-DshDataHomeFromCommandLines @($shaped))) "got '$((Get-DshDataHomeFromCommandLines @($shaped)))'"
} finally {
    Remove-Item $detectStage -Recurse -Force -ErrorAction SilentlyContinue
}

# The two must not grow separate copies of the rule again.
$bootstrapSource = [IO.File]::ReadAllText((Join-Path $root 'scripts\bootstrap.ps1'), [Text.Encoding]::UTF8)
Check 'bootstrap calls the shared detection helper' `
    ($bootstrapSource -match 'Get-DshDataHomeFromCommandLines') 'bootstrap no longer detects on its own'
Check 'bootstrap carries no command-line pattern of its own' `
    (-not ($bootstrapSource -match 'node_modules..@deepseek-ai')) 'a second copy of the rule is back'
$rustSource = [IO.File]::ReadAllText((Join-Path $root 'crates\handoff-core\src\config.rs'), [Text.Encoding]::UTF8)
Check 'the Rust side keeps the rule in a testable function' `
    ($rustSource -match 'pub fn dsh_install_root') 'detect_dsh parses inline again'

Write-Output ''
Write-Output "Bootstrap/first-run: $checks check(s), $($failures.Count) failure(s)."
if ($failures.Count) {
    $failures | ForEach-Object { Write-Error "failed: $_" }
    exit 1
}
Write-Output 'All bootstrap/first-run checks passed. The real runtime/ directory was not touched.'
