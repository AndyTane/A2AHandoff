# Read-only coherence check for a RUNNING A2AHandoff instance.
#
# Answers "is the thing actually healthy right now?" as a set of asserted invariants
# rather than a wall of numbers to eyeball. It reads state and runs the observers; it
# never writes runtime state, never clicks, and never sends anything.
#
# Exit 0 = every invariant held, or no instance is running (skipped).
# Exit 1 = at least one violation (printed).
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File tests\test-runtime-coherence.ps1
#   ... -SkipObservers     # state-only, fast, does not touch Claude/DSH (CI-safe)
#   ... -RequireRunning    # fail instead of skipping when no instance is up
[CmdletBinding()]
param(
    [string]$Root = '',
    [switch]$SkipObservers,
    [int]$StaleSeconds = 10,
    [switch]$RequireRunning
)
$ErrorActionPreference = 'Stop'
# `$PSScriptRoot` is not populated inside a param() default when run with -File, so
# resolve the product root here instead.
if (-not $Root) { $Root = (Resolve-Path (Join-Path (Split-Path $PSCommandPath -Parent) '..')).Path }
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

$fail = [Collections.Generic.List[string]]::new()
$pass = 0
function Check($name, $ok, $detail = '') {
    if ($ok) { $script:pass++; Write-Output "  ok    $name" }
    else { $script:fail.Add("$name $detail"); Write-Output "  FAIL  $name $detail" }
}
function Read-Json($p) {
    if (-not (Test-Path $p)) { return $null }
    try { return [IO.File]::ReadAllText($p, [Text.Encoding]::UTF8) | ConvertFrom-Json } catch { return $null }
}

$rt = Join-Path $Root 'runtime'
$state = Read-Json (Join-Path $rt 'state.json')
$wf = Read-Json (Join-Path $rt 'workflow.json')
$cfg = Read-Json (Join-Path $rt 'config.json')

# No instance running: this suite has nothing to assert, so skip rather than fail - CI
# and a fresh clone both land here.
if ($null -eq $state -or $null -eq $wf) {
    Write-Output 'No runtime state present: nothing is running, so coherence is not applicable.'
    if ($RequireRunning) {
        Write-Output '{"tests_passed":0,"messages_sent":0,"skipped":true,"error":"no instance running"}'
        exit 1
    }
    Write-Output '{"tests_passed":0,"messages_sent":0,"skipped":true,"failures":0}'
    exit 0
}

Write-Output "=== files present ==="
Check 'state.json readable' ($null -ne $state)
Check 'workflow.json readable' ($null -ne $wf)
if ($null -eq $state -or $null -eq $wf) { Write-Output ''; Write-Output 'cannot continue without state'; exit 1 }

Write-Output ''
Write-Output "=== runtime liveness ==="
$age = [math]::Round(([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() - [long]$state.at_ms) / 1000, 1)
Check "state.json is fresh (${age}s < ${StaleSeconds}s)" ($age -lt $StaleSeconds) "age=${age}s"
$alive = $null -ne (Get-Process -Id ([int]$state.pid) -ErrorAction SilentlyContinue)
Check "runtime pid $($state.pid) is alive" $alive
$ui = @(Get-Process A2AHandoff -ErrorAction SilentlyContinue)
Check 'exactly one A2AHandoff UI process' ($ui.Count -eq 1) "count=$($ui.Count)"

Write-Output ''
Write-Output "=== phase is one the runtime actually publishes ==="
$known = @('unclaimed', 'waiting_dsh', 'waiting_claude', 'idle', 'paused_by_user', 'hold_preparation', 'hold_send_uncertain')
Check "phase '$($state.phase)' is a known phase" ($known -contains $state.phase)

if (-not $SkipObservers) {
    Write-Output ''
    Write-Output "=== observation ==="
    $dsh = $null; $claude = $null
    try { $dsh = (& node.exe (Join-Path $Root 'adapters\dsh\observe.mjs') $Root) | ConvertFrom-Json } catch { }
    try {
        $b = Read-Json (Join-Path $rt 'bindings.json')
        $o = Join-Path $env:TEMP ("coh-" + [guid]::NewGuid().ToString('N') + ".json")
        & powershell.exe -NoProfile -STA -ExecutionPolicy Bypass -File (Join-Path $Root 'adapters\claude\observe.ps1') `
            -SessionId $b.claude_session -Title $b.claude_window -ClaudeHost claude.ai 1>$o 2>$null
        if (Test-Path $o) { $claude = [IO.File]::ReadAllText($o, [Text.Encoding]::UTF8) | ConvertFrom-Json; Remove-Item $o -Force }
    } catch { }

    Check 'DSH observer returns ok' ($null -ne $dsh -and $dsh.ok)
    Check 'Claude observer returns ok' ($null -ne $claude -and $claude.ok)

    if ($null -ne $dsh -and $dsh.ok -and $null -ne $claude -and $claude.ok) {
        # The status text must not name a side as working while that side is idle. The
        # claim is deliberately narrow: a pending reply MAY be described as waiting for
        # the side that produced it, so 「等待 Claude 回复」 while Claude sits at `replied`
        # is correct and must not be flagged.
        $busyClaude = $claude.state -in @('running', 'awaiting_reply', 'needs_input')
        $st = [string]$state.status_text
        Write-Output ''
        Write-Output "=== status text names the working side ==="
        Write-Output "  (dsh.busy=$($dsh.busy)  claude.state=$($claude.state)  status='$st')"
        if ($state.sending) {
            Check 'while sending, status is a sending state' ($st -match '正在发送|正在填入')
        } else {
            if ($dsh.busy) {
                Check 'DSH is executing -> status says DSH' ($st -like '*DSH*') "got '$st'"
            } else {
                Check 'DSH idle -> status does NOT claim DSH is executing' `
                    ($st -notlike '*等待 DSH 执行结果*') "got '$st'"
            }
            if ($busyClaude) {
                Check 'Claude is generating -> status says Claude' ($st -like '*Claude*') "got '$st'"
            }
        }

        # Watermarks must not sit ahead of what actually happened.
        Check 'dsh_after_seq <= live last_seq' ([long]$wf.dsh_after_seq -le [long]$dsh.last_seq) `
            "after=$($wf.dsh_after_seq) last=$($dsh.last_seq)"
        if ($null -ne $dsh.result) {
            # Three legitimate outcomes, not two: delivered, still pending, or held with the
            # runtime stopped. A `draft_unverified`/`submit_uncertain` hold is deliberately
            # unresolved - the window says so and the runtime refuses to resend blind - so
            # counting it as "the result vanished" was a false alarm.
            $held = ($wf.phase -eq 'hold_send_uncertain') -and
                (@('draft_unverified', 'submit_uncertain', 'send_attempted') -contains [string]$wf.last_delivery.state)
            Check 'a completed result is delivered, pending or explicitly held' `
                ((([long]$dsh.result.end_seq -le [long]$wf.dsh_after_seq) -or ($null -ne $wf.pending) -or $held)) `
                "end_seq=$($dsh.result.end_seq) after=$($wf.dsh_after_seq) phase=$($wf.phase) last=$($wf.last_delivery.state)"
        }
        Check 'claude_floor <= live ui_message_index' ([long]$wf.claude_floor -le [long]$claude.ui_message_index) `
            "floor=$($wf.claude_floor) ui=$($claude.ui_message_index)"
    }
}

Write-Output ''
Write-Output "=== receipts are in a known state ==="
$terminal = @('sent', 'submit_uncertain')
$retryable = @('draft_ready', 'cancelled_before_send', 'draft_write_attempted', 'draft_unverified', 'send_attempted')
# States earlier builds wrote and the current code no longer does. They are history, not
# corruption, so they are recognised but called out separately.
$legacy = @('paste_attempted')
$rc = @(Get-ChildItem (Join-Path $rt 'receipts\*.json') -ErrorAction SilentlyContinue)
$odd = @(); $old = 0
foreach ($f in $rc) {
    $j = Read-Json $f.FullName
    if ($null -eq $j) { $odd += "$($f.Name):unreadable"; continue }
    if ($j.state -in $legacy) { $old++; continue }
    if ($j.state -notin ($terminal + $retryable)) { $odd += "$($f.Name):$($j.state)" }
}
Check "all $($rc.Count) receipts have a known state" ($odd.Count -eq 0) ($odd -join ' ')
if ($old) { Write-Output "  note  $old receipt(s) use the retired state 'paste_attempted' (pre-dates this build)" }

Write-Output ''
Write-Output "=== no stuck pending ==="
if ($null -ne $wf.pending) {
    $p = $wf.pending
    Write-Output "  pending: stage=$($p.stage) dir=$($p.direction) created_ms=$($p.created_ms)"
    Check 'pending stage is known' ($p.stage -in @('queued', 'draft_ready', 'waiting_target'))
    if ($p.stage -in @('queued', 'waiting_target')) {
        # The countdown only starts once the adapter has VERIFIED the write, so a pending
        # handoff legitimately has no deadline while it is still being prepared, or while
        # it waits for the target composer to become available. Age alone therefore does
        # not prove a stall: an adapter retrying against a busy or wrong tab looks the same
        # from here. Distinguish them by whether the adapter is still TRYING - recent
        # preparation_deferred events mean it is working, and the last one carries the
        # reason (e.g. the target tab is not in front).
        $ageS = [math]::Round(([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() - [long]$p.created_ms) / 1000, 1)
        $recent = $null
        $log = Join-Path $rt 'events.jsonl'
        if (Test-Path $log) {
            $cut = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() - 90000
            foreach ($line in (Get-Content $log -Tail 60)) {
                try { $e = $line | ConvertFrom-Json } catch { continue }
                if ($e.phase -eq 'preparation_deferred' -and [long]$e.at_ms -gt $cut) { $recent = $e; break }
            }
        }
        if ($recent) {
            Check "a waiting handoff is still being retried (${ageS}s, adapter active)" $true
            $why = [string]$recent.data.error
            if ($why.Length -gt 110) { $why = $why.Substring(0, 110) }
            Write-Output "  note  last deferral: $why"
            if ($why -like '*TARGET_WINDOW_UNAVAILABLE*') {
                Write-Output "  note  the browser's ACTIVE tab is a different session than the one"
                Write-Output "        being observed - bring the tab named in expects= to the front."
            }
        } else {
            Check "a pending handoff has not been abandoned (${ageS}s, no adapter activity)" ($ageS -lt 120) `
                "stage=$($p.stage), no preparation attempt in 90s - adapter may have stopped"
        }
    } else {
        Check 'a prepared handoff carries a countdown deadline' ([long]$p.deadline_ms -gt 0) `
            "stage=$($p.stage) deadline_ms=$($p.deadline_ms)"
    }
} else {
    Check 'no pending handoff is parked' $true
}

Write-Output ''
Write-Output "=== no crash since this runtime started ==="
# runtime-stderr.log is append-only and shared across runs, so its SIZE says nothing
# about the current run. Compare its last write against the runtime's start instead.
$errLog = Join-Path $rt 'runtime-stderr.log'
$runStart = ($ui | Sort-Object StartTime | Select-Object -First 1).StartTime
if (-not $runStart) { $runStart = [DateTimeOffset]::FromUnixTimeMilliseconds([long]$state.at_ms).LocalDateTime }
if (Test-Path $errLog) {
    $wrote = (Get-Item $errLog).LastWriteTime
    Write-Output "  runtime started $($runStart.ToString('HH:mm:ss'))  |  stderr last written $($wrote.ToString('HH:mm:ss'))"
    Check 'no crash line was written by this run' ($wrote -lt $runStart) `
        "stderr written at $($wrote.ToString('HH:mm:ss')), after the $($runStart.ToString('HH:mm:ss')) start"
} else {
    Check 'no crash line was written by this run' $true
}

Write-Output ''
Write-Output "coherence: $pass passed, $($fail.Count) failed"
if ($fail.Count) {
    Write-Output ''
    Write-Output 'VIOLATIONS:'
    $fail | ForEach-Object { Write-Output "  - $_" }
    exit 1
}
Write-Output 'All coherence invariants held. Nothing was written or sent.'
