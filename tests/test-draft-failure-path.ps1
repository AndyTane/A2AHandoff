# The adapter must answer even when it fails, and must refuse the right things.
#
# draft-flow.ps1 runs under `Set-StrictMode -Version Latest` (draft-primitives.ps1). When its
# error handler read a variable that had never been assigned, the handler itself threw, the
# adapter printed nothing, and the runtime could only report
#   adapter_output_invalid: ...\adapters\windows\draft-flow.ps1
# - a failure naming the file but not the cause, and leaving no receipt at all. So the first
# contract is: a refused request still produces exactly one JSON object naming a cause, and a
# non-zero exit.
#
# The second contract is the retry rule. 「重试本次投递」 may re-run only a draft that could not
# have reached the peer (`draft_unverified`, written before the submit step) and only for a
# deliberate click; anything that might already be delivered stays refused.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

$root = Split-Path $PSScriptRoot -Parent
$adapter = Join-Path $root 'adapters\windows\draft-flow.ps1'
$requestId = 'aaaaaaaaaaaaaaaaaaaaaaaa'
$text = 'compliance check'
$passed = [Collections.Generic.List[string]]::new()
function Check($name, $condition, $detail) {
    if (-not $condition) { throw ('FAILED ' + $name + ': ' + $detail) }
    $passed.Add($name)
}

# Runs the adapter once against a throwaway root holding `$state` as this delivery's receipt.
function Invoke-Draft {
    param([string]$State, [bool]$Manual)
    $scratch = Join-Path ([IO.Path]::GetTempPath()) ('a2a-draft-' + [Guid]::NewGuid().ToString('N'))
    try {
        foreach ($d in @('runtime\receipts', 'runtime\requests', 'runtime\commands')) {
            New-Item -ItemType Directory -Force -Path (Join-Path $scratch $d) | Out-Null
        }
        $requestFile = Join-Path $scratch ('runtime\requests\' + $requestId + '.json')
        @{
            id = $requestId; flow = 'draft-first-v1'; message_contract = 'configured-plain-v2'
            direction = 'DSH_TO_CLAUDE'; text = $text; source_hash = 'x'; source_seq = 1
            source_turn = 1; claude_session = 'cse_x'; dsh_session = 'session-x'
            manual = $Manual; dispatch_delay_seconds = 0
        } | ConvertTo-Json -Compress | Set-Content $requestFile -Encoding UTF8
        # A pending that is NOT this request: whatever gets past the receipt guard stops here.
        '{"pending":{"id":"bbbbbbbbbbbbbbbbbbbbbbbb"}}' |
            Set-Content (Join-Path $scratch 'runtime\workflow.json') -Encoding UTF8
        '{"claude_session":"cse_x","dsh_session":"session-x"}' |
            Set-Content (Join-Path $scratch 'runtime\bindings.json') -Encoding UTF8
        '{"enabled":true}' | Set-Content (Join-Path $scratch 'runtime\config.json') -Encoding UTF8
        if ($State) {
            @{
                id = $requestId; flow = 'draft-first-v1'; state = $State
                direction = 'DSH_TO_CLAUDE'; source_seq = 1; source_turn = 1
                claude_session = 'cse_x'; dsh_session = 'session-x'
                outgoing_hash = 'deadbeef'; at_ms = 1; draft_ready_at_ms = 0
                deadline_ms = 0; anchor = $null
            } | ConvertTo-Json -Compress |
                Set-Content (Join-Path $scratch ('runtime\receipts\' + $requestId + '.json')) -Encoding UTF8
        }
        $output = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $adapter `
            -ProductRoot $scratch -RequestFile $requestFile -Operation Prepare 2>&1
        $code = $LASTEXITCODE
        $receiptWritten = Test-Path -LiteralPath (Join-Path $scratch ('runtime\receipts\' + $requestId + '.json'))
    } finally {
        Remove-Item -Recurse -Force $scratch -ErrorAction SilentlyContinue
    }
    $line = @($output | Where-Object { $_ -like '{*' } | Select-Object -Last 1)
    [pscustomobject]@{
        Exit = $code
        Raw = ($output -join ' ')
        Receipt = $receiptWritten
        Json = if ($line.Count -eq 1) { $line[0] | ConvertFrom-Json } else { $null }
    }
}

# 1. A refused request still answers, with a cause and a non-zero exit.
$first = Invoke-Draft -State '' -Manual $false
Check 'a_refused_request_exits_non_zero' ($first.Exit -ne 0) "exit=$($first.Exit)"
Check 'the_adapter_answers_with_json' ($null -ne $first.Json) "output=$($first.Raw)"
Check 'the_answer_says_it_failed' ($first.Json.ok -eq $false) "ok=$($first.Json.ok)"
Check 'the_answer_names_a_cause' (-not [string]::IsNullOrWhiteSpace([string]$first.Json.error)) `
    "error='$($first.Json.error)'"
Check 'a_refusal_leaves_no_receipt' (-not $first.Receipt) 'a refused request must not write a receipt'

# 2. An automatic request never re-runs an unverified draft: only a click may.
$auto = Invoke-Draft -State 'draft_unverified' -Manual $false
Check 'an_automatic_retry_of_an_unverified_draft_is_refused' `
    ($auto.Json.error -eq 'DELIVERY_ALREADY_ATTEMPTED_NO_RETRY') "error=$($auto.Json.error)"

# 3. Neither does a click, once the draft might have gone out.
foreach ($state in @('send_attempted', 'submit_uncertain', 'sent')) {
    $r = Invoke-Draft -State $state -Manual $true
    Check ("a_click_must_not_re_run_$state") `
        ($r.Json.error -eq 'DELIVERY_ALREADY_ATTEMPTED_NO_RETRY') "error=$($r.Json.error)"
}

# 4. A click on an unverified draft passes the guard: it fails later, for another reason.
$retry = Invoke-Draft -State 'draft_unverified' -Manual $true
Check 'a_click_may_re_run_an_unverified_draft' `
    ($retry.Json.error -ne 'DELIVERY_ALREADY_ATTEMPTED_NO_RETRY') `
    "error=$($retry.Json.error)"

# 5. A cancelled draft never left either, so a click may re-run it - but 取消本次 must keep
#    blocking the automatic path, or the cancel means nothing.
$cancelled = Invoke-Draft -State 'cancelled_before_send' -Manual $true
Check 'a_click_may_re_run_a_cancelled_draft' `
    ($cancelled.Json.error -ne 'DELIVERY_ALREADY_ATTEMPTED_NO_RETRY') `
    "error=$($cancelled.Json.error)"
$cancelledAuto = Invoke-Draft -State 'cancelled_before_send' -Manual $false
Check 'a_cancel_still_blocks_the_automatic_path' `
    ($cancelledAuto.Json.error -eq 'DELIVERY_ALREADY_ATTEMPTED_NO_RETRY') `
    "error=$($cancelledAuto.Json.error)"

Write-Output ("draft failure path: {0} passed, 0 failed" -f $passed.Count)
Write-Output ("  refusal cause: {0}; retry stop: {1}" -f $first.Json.error, $retry.Json.error)
Write-Output 'No messages were sent.'
