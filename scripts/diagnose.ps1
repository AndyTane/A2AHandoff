# Why is it not working? One read-only answer, in layers.
#
# `scripts/check-prereqs.ps1` sees the environment and `tests/test-runtime-coherence.ps1` sees a
# running instance - and that suite exits 0 when there is no instance, so a product that is
# missing files, has never been configured or has never been started looks exactly like a
# healthy one. This is the doctor that covers those, and it is the one command to run first.
#
# It never writes runtime state, never clicks and never sends. Everything it reads is already on
# disk. Exit 0 = nothing wrong (warnings may still be listed), 1 = a real problem, 2 = the
# product folder itself is incomplete.
[CmdletBinding()]
param(
    # The product folder: the one holding A2AHandoff.exe. Defaults to this script's parent.
    [string]$Root = '',
    # Machine-readable, for an agent: one JSON object with the findings and the next step.
    [switch]$Json,
    # Skip the environment layer, which starts a child process and opens a TCP connection.
    [switch]$SkipEnvironment
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

if (-not $Root) { $Root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path }
$Root = [IO.Path]::GetFullPath($Root)
$runtime = Join-Path $Root 'runtime'

$findings = [Collections.Generic.List[object]]::new()
function Finding($Layer, $Name, $State, $Detail = '') {
    $findings.Add([pscustomobject]@{ layer = $Layer; name = $Name; state = $State; detail = $Detail })
}
# Read without holding the file open: the runtime publishes by temp-file-and-rename, and a
# reader that keeps the handle makes the rename fail on Windows.
function Read-Json($Path) {
    if (-not (Test-Path -LiteralPath $Path)) { return $null }
    try {
        $fs = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read,
            [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete)
        $sr = New-Object IO.StreamReader($fs, [Text.Encoding]::UTF8)
        $text = $sr.ReadToEnd(); $sr.Close(); $fs.Close()
        return ($text | ConvertFrom-Json)
    } catch {
        return 'UNPARSEABLE'
    }
}
function Read-Text($Path) {
    if (-not (Test-Path -LiteralPath $Path)) { return $null }
    try {
        $fs = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read,
            [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete)
        $sr = New-Object IO.StreamReader($fs, [Text.Encoding]::UTF8)
        $text = $sr.ReadToEnd(); $sr.Close(); $fs.Close()
        return $text
    } catch { return $null }
}

# ---- 1. install: is what the tool needs actually here? -------------------------------------
# The executables sit in the product root in a release package, but under target\release (or
# dist\) in a source tree - `--product-root .` is used both ways, so both layouts count.
function Resolve-Exe($Name) {
    foreach ($candidate in @($Name, "target\release\$Name", "dist\$Name")) {
        $full = Join-Path $Root $candidate
        if (Test-Path -LiteralPath $full) { return $full }
    }
    return $null
}
# Every entry carries every key: under Set-StrictMode, reading a missing hashtable key is fatal.
foreach ($required in @(
        @{ name = 'A2AHandoff.exe'; what = 'the app'; path = $null },
        @{ name = 'handoff-runtime.exe'; what = 'the runtime it drives'; path = $null },
        @{ name = 'draft-flow.ps1'; what = 'the delivery adapter'; path = (Join-Path $Root 'adapters\windows\draft-flow.ps1') },
        @{ name = 'observe.ps1'; what = 'the Claude reader'; path = (Join-Path $Root 'adapters\claude\observe.ps1') },
        @{ name = 'observe.mjs'; what = 'the DSH reader'; path = (Join-Path $Root 'adapters\dsh\observe.mjs') })) {
    $full = if ($required.path) { $required.path } else { Resolve-Exe $required.name }
    $present = $null -ne $full
    Finding 'install' $(if ($required.name) { $required.name } else { Split-Path $required.path -Leaf }) `
        $(if ($present) { 'ok' } else { 'fail' }) `
        $(if ($present) { '' } else { "missing: $($required.what) - the product folder is incomplete" })
}
# bootstrap's own inputs: the app provisions itself without them, bootstrap does not.
foreach ($optional in @('examples\message-templates.json', 'scripts\bootstrap.ps1')) {
    $present = Test-Path -LiteralPath (Join-Path $Root $optional)
    Finding 'install' $optional $(if ($present) { 'ok' } else { 'warn' }) `
    $(if ($present) { '' } else { 'missing: scripts\bootstrap.ps1 will not run' })
}

$config = Read-Json (Join-Path $runtime 'config.json')
if ($null -eq $config) {
    Finding 'install' 'runtime\config.json' 'warn' 'not created yet: the app writes it on first start, or run scripts\bootstrap.ps1'
} elseif ($config -eq 'UNPARSEABLE') {
    Finding 'install' 'runtime\config.json' 'fail' 'exists but cannot be parsed - delete it and run scripts\bootstrap.ps1'
} else {
    Finding 'install' 'runtime\config.json' 'ok' ''
    $dataHome = [string]$config.dsh_data_home
    if ([string]::IsNullOrWhiteSpace($dataHome)) {
        Finding 'install' 'dsh_data_home' 'warn' 'empty: DSH sessions cannot be discovered'
    } elseif (-not (Test-Path -LiteralPath (Join-Path $dataHome 'storages\session_projcache\sessions'))) {
        Finding 'install' 'dsh_data_home' 'fail' "does not look like a DSH data directory: $dataHome"
    } else {
        Finding 'install' 'dsh_data_home' 'ok' $dataHome
    }
}

# Placeholders are what a person sees on a product nobody has bound yet - and the shipped
# examples ARE that set, so the comparison cannot drift from the constants in the code.
$bindings = Read-Json (Join-Path $runtime 'bindings.json')
$placeholderSet = Read-Json (Join-Path $Root 'examples\bindings.json')
$unbound = @()
if ($null -eq $bindings) {
    Finding 'install' 'runtime\bindings.json' 'warn' 'not created yet: nothing is bound'
} elseif ($bindings -eq 'UNPARSEABLE') {
    Finding 'install' 'runtime\bindings.json' 'fail' 'exists but cannot be parsed'
} else {
    if ($placeholderSet -and $placeholderSet -ne 'UNPARSEABLE') {
        if ([string]$bindings.claude_session -eq [string]$placeholderSet.claude_session) { $unbound += 'Claude' }
        if ([string]$bindings.dsh_session -eq [string]$placeholderSet.dsh_session) { $unbound += 'DSH' }
    }
    if ($unbound.Count) {
        Finding 'install' 'bindings' 'warn' "still the shipped placeholders: $($unbound -join ' and ') not bound"
    } else {
        Finding 'install' 'bindings' 'ok' "Claude $([string]$bindings.claude_session) · DSH $([string]$bindings.dsh_session)"
    }
}

# ---- 2. environment: can this machine run it at all? ---------------------------------------
if ($SkipEnvironment) {
    Finding 'env' 'checks' 'skip' 'skipped by -SkipEnvironment'
} else {
    $prereqs = Join-Path $Root 'scripts\check-prereqs.ps1'
    if (-not (Test-Path -LiteralPath $prereqs)) {
        Finding 'env' 'check-prereqs.ps1' 'skip' 'not present in this folder'
    } else {
        $raw = & powershell -NoProfile -ExecutionPolicy Bypass -File $prereqs -Json 2>&1
        $parsed = $null
        try { $parsed = ($raw | Where-Object { $_ -like '[*' } | Select-Object -Last 1) | ConvertFrom-Json } catch { }
        if ($null -eq $parsed) {
            Finding 'env' 'checks' 'warn' 'check-prereqs.ps1 did not return a readable result'
        } else {
            foreach ($c in @($parsed)) {
                if (-not $c.ok) { Finding 'env' $c.name 'fail' [string]$c.detail }
            }
            if (@($parsed | Where-Object { -not $_.ok }).Count -eq 0) {
                Finding 'env' 'checks' 'ok' "$(@($parsed).Count) prerequisite check(s) passed"
            }
        }
    }
}

# ---- 3. runtime: is it alive, and in a state the product knows? ----------------------------
$state = Read-Json (Join-Path $runtime 'state.json')
$ui = @(Get-Process A2AHandoff -ErrorAction SilentlyContinue)
if ($null -eq $state) {
    Finding 'runtime' 'state.json' 'warn' 'no state: the app has not run here yet (or runtime\ was cleared)'
} elseif ($state -eq 'UNPARSEABLE') {
    Finding 'runtime' 'state.json' 'fail' 'exists but cannot be parsed - a write was interrupted'
} else {
    $age = [math]::Round(([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() - [long]$state.at_ms) / 1000, 1)
    Finding 'runtime' 'state freshness' $(if ($age -lt 45) { 'ok' } else { 'fail' }) "${age}s old"
    $runPid = [int]$state.pid
    $alive = $null -ne (Get-Process -Id $runPid -ErrorAction SilentlyContinue)
    Finding 'runtime' 'runtime process' $(if ($alive) { 'ok' } else { 'fail' }) "pid $runPid alive=$alive"
    Finding 'runtime' 'app process' $(if ($ui.Count -eq 1) { 'ok' } else { 'warn' }) "$($ui.Count) A2AHandoff process(es)"
    $known = @('unclaimed', 'waiting_dsh', 'waiting_claude', 'idle', 'paused_by_user', 'hold_preparation',
        'hold_send_uncertain')
    $phase = [string]$state.phase
    Finding 'runtime' 'phase' $(if ($known -contains $phase) { 'ok' } else { 'fail' }) "'$phase'"
    $last = $state.last_delivery
    if ($null -ne $last -and -not [string]::IsNullOrWhiteSpace([string]$last.state)) {
        $terminal = @('sent', 'submit_uncertain') -contains [string]$last.state
        Finding 'runtime' 'last delivery' $(if ($terminal) { 'ok' } else { 'warn' }) `
            "$([string]$last.state) / $([string]$last.direction)"
    }
    if ($state.sending) { Finding 'runtime' 'sending' 'warn' 'a submit is in flight right now' }
}

# ---- 4. history: did this run crash, and what has been refused? ----------------------------
$errLog = Join-Path $runtime 'runtime-stderr.log'
if (Test-Path -LiteralPath $errLog) {
    $wrote = (Get-Item $errLog).LastWriteTime
    # The log is append-only and shared across runs, so its size says nothing about this one.
    # Compare its last write against the start of the processes that are up.
    $runStart = ($ui | Sort-Object StartTime | Select-Object -First 1).StartTime
    if ($runStart -and $wrote -ge $runStart) {
        Finding 'history' 'runtime-stderr.log' 'fail' "written at $($wrote.ToString('HH:mm:ss')), after the $($runStart.ToString('HH:mm:ss')) start - read it"
    } else {
        Finding 'history' 'runtime-stderr.log' 'ok' "last written $(if ($runStart) { $wrote.ToString('HH:mm:ss') } else { 'before this run' })"
    }
} else {
    Finding 'history' 'runtime-stderr.log' 'ok' 'nothing has been written to it'
}
$events = Read-Text (Join-Path $runtime 'events.jsonl')
if ($events) {
    $lines = @($events -split "`n" | Where-Object { $_.Trim() })
    Finding 'history' 'events.jsonl' 'ok' "$($lines.Count) event(s)"
    $refusals = @($lines | Select-Object -Last 50 | Where-Object { $_ -match 'send_refused|preparation_failed|submit_uncertain' })
    if ($refusals.Count) {
        Finding 'history' 'recent refusals' 'warn' "$($refusals.Count) in the last 50 events: the newest is $($refusals[-1])"
    }
} else {
    Finding 'history' 'events.jsonl' 'skip' 'no events recorded'
}

# ---- 5. both sides: what does the runtime say about the two sessions? ----------------------
if ($state -and $state -ne 'UNPARSEABLE') {
    $detail = [string]$state.detail
    foreach ($side in @(@{ key = 'claude_ok'; who = 'Claude' }, @{ key = 'dsh_ok'; who = 'DSH' })) {
        $ok = $state.$($side.key)
        if ($null -eq $ok) {
            Finding 'sides' $side.who 'skip' 'the runtime has not reported on it yet'
        } elseif ($ok) {
            Finding 'sides' $side.who 'ok' "turn $($state.reply_turn)/$($state.dsh_turn)"
        } else {
            Finding 'sides' $side.who 'warn' $(if ($detail) { $detail } else { 'the runtime cannot read it' })
        }
    }
} else {
    Finding 'sides' 'both' 'skip' 'no published state to read'
}

# ---- the verdict: one step, chosen by what would unblock the most --------------------------
$fails = @($findings | Where-Object { $_.state -eq 'fail' })
$installFails = @($fails | Where-Object { $_.layer -eq 'install' })
$exit = if ($installFails.Count) { 2 } elseif ($fails.Count) { 1 } else { 0 }

$next = 'No problem found.'
if ($installFails.Count) {
    $next = "Re-unpack the whole archive into one folder - these are missing: $((@($installFails | ForEach-Object { $_.name }) -join ', ')). The executables load adapters\ from beside them."
} elseif ($unbound.Count) {
    $next = "Open 绑定配置 in the window and bind the session(s) still on the shipped placeholder: $($unbound -join ' and '). Choosing them is the user's decision."
} elseif (@($findings | Where-Object { $_.layer -eq 'install' -and $_.state -eq 'warn' -and $_.name -eq 'runtime\config.json' }).Count) {
    $next = 'Nothing is configured yet: start A2AHandoff.exe (it writes runtime\config.json itself) or run scripts\bootstrap.ps1.'
} elseif (@($findings | Where-Object { $_.layer -eq 'env' -and $_.state -eq 'fail' }).Count) {
    $next = "Fix the failed prerequisite(s) above first: $((@($findings | Where-Object { $_.layer -eq 'env' -and $_.state -eq 'fail' } | ForEach-Object { $_.name }) -join ', '))."
} elseif (@($findings | Where-Object { $_.name -eq 'state freshness' -and $_.state -eq 'fail' }).Count -or
    @($findings | Where-Object { $_.name -eq 'runtime process' -and $_.state -eq 'fail' }).Count) {
    $next = 'The runtime has stopped publishing: read runtime\runtime-stderr.log, then restart A2AHandoff.exe. Session state is not lost by a restart.'
} elseif (@($findings | Where-Object { $_.name -eq 'runtime-stderr.log' -and $_.state -eq 'fail' }).Count) {
    $next = 'This run wrote to runtime\runtime-stderr.log: read its last lines.'
} elseif (@($findings | Where-Object { $_.name -eq 'state.json' -and $_.state -eq 'warn' }).Count) {
    $next = 'Start A2AHandoff.exe, then run this again.'
} elseif ($null -ne $state -and $state -ne 'UNPARSEABLE' -and [string]$state.phase -eq 'hold_preparation') {
    $next = 'A delivery is held on an occupied input: clear the target input box, then press 重试本次投递.'
} elseif ($null -ne $state -and $state -ne 'UNPARSEABLE' -and [string]$state.last_delivery.state -eq 'submit_uncertain') {
    $next = 'The last submit was never confirmed. It is never retried automatically - decide whether to resend, and check the target conversation first.'
} elseif (@($findings | Where-Object { $_.layer -eq 'sides' -and $_.state -eq 'warn' }).Count) {
    $next = 'The runtime cannot read a session: open the bound conversation (Claude Desktop / the DSH web UI) and press 立即轮询.'
} elseif ($null -ne $state -and $state -ne 'UNPARSEABLE') {
    $next = "Nothing to fix. Currently: $([string]$state.phase) - $([string]$state.status_text)"
}

if ($Json) {
    ([pscustomobject]@{
            schema   = 1
            product  = $Root
            verdict  = $(if ($exit -eq 2) { 'install_incomplete' } elseif ($exit -eq 1) { 'problem' } else { 'ok' })
            next     = $next
            failures = $fails.Count
            findings = @($findings)
            messages_sent = 0
        } | ConvertTo-Json -Depth 6)
    exit $exit
}

$layerNames = [ordered]@{ install = 'Install'; env = 'Environment'; runtime = 'Runtime'; history = 'History'; sides = 'Both sides' }
foreach ($layer in $layerNames.Keys) {
    Write-Output "=== $($layerNames[$layer]) ==="
    foreach ($f in @($findings | Where-Object { $_.layer -eq $layer })) {
        $mark = switch ($f.state) { 'ok' { 'ok  ' } 'warn' { 'warn' } 'fail' { 'FAIL' } default { '--  ' } }
        $line = "  $mark $($f.name)"
        if ($f.detail) { $line += "  -  $($f.detail)" }
        Write-Output $line
    }
    Write-Output ''
}
Write-Output "下一步: $next"
Write-Output ''
Write-Output "diagnose: $($fails.Count) failure(s), $(@($findings | Where-Object { $_.state -eq 'warn' }).Count) warning(s). Nothing was written and no message was sent."
exit $exit
