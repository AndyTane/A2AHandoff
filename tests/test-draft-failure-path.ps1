# The adapter must answer even when it fails.
#
# draft-flow.ps1 runs under `Set-StrictMode -Version Latest` (draft-primitives.ps1). When its
# error handler read a variable that had never been assigned, the handler itself threw, the
# adapter printed nothing, and the runtime could only report
#   adapter_output_invalid: ...\adapters\windows\draft-flow.ps1
# - a failure naming the file but not the cause, and leaving no receipt at all. This pins the
# contract: a refused request still produces exactly one JSON object on stdout and a non-zero
# exit code.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

$root = Split-Path $PSScriptRoot -Parent
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('a2a-draft-failure-' + [Guid]::NewGuid().ToString('N'))
$requestId = 'aaaaaaaaaaaaaaaaaaaaaaaa'
$passed = [Collections.Generic.List[string]]::new()
function Check($name, $condition, $detail) {
    if (-not $condition) { throw ('FAILED ' + $name + ': ' + $detail) }
    $passed.Add($name)
}

try {
    foreach ($d in @('runtime\receipts', 'runtime\requests', 'runtime\commands')) {
        New-Item -ItemType Directory -Force -Path (Join-Path $scratch $d) | Out-Null
    }
    $requestFile = Join-Path $scratch ('runtime\requests\' + $requestId + '.json')
    # A well-formed request whose id is NOT the workflow's pending one: the adapter must
    # refuse it early, before touching any window.
    @'
{"id":"aaaaaaaaaaaaaaaaaaaaaaaa","flow":"draft-first-v1","message_contract":"configured-plain-v2",
 "direction":"DSH_TO_CLAUDE","text":"compliance check","source_hash":"x","source_seq":1,
 "source_turn":1,"claude_session":"cse_x","dsh_session":"session-x","manual":false}
'@ | Set-Content $requestFile -Encoding UTF8
    '{"pending":{"id":"bbbbbbbbbbbbbbbbbbbbbbbb"}}' |
        Set-Content (Join-Path $scratch 'runtime\workflow.json') -Encoding UTF8
    '{"claude_session":"cse_x","dsh_session":"session-x"}' |
        Set-Content (Join-Path $scratch 'runtime\bindings.json') -Encoding UTF8
    '{"enabled":true}' | Set-Content (Join-Path $scratch 'runtime\config.json') -Encoding UTF8

    $adapter = Join-Path $root 'adapters\windows\draft-flow.ps1'
    $output = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $adapter `
        -ProductRoot $scratch -RequestFile $requestFile -Operation Prepare 2>&1
    $code = $LASTEXITCODE

    Check 'a_refused_request_exits_non_zero' ($code -ne 0) "exit=$code output=$output"
    $line = @($output | Where-Object { $_ -like '{*' } | Select-Object -Last 1)
    Check 'the_adapter_answers_with_json' ($line.Count -eq 1) "output=$output"
    $json = $line[0] | ConvertFrom-Json
    Check 'the_answer_says_it_failed' ($json.ok -eq $false) "ok=$($json.ok)"
    Check 'the_answer_names_a_cause' (-not [string]::IsNullOrWhiteSpace([string]$json.error)) `
        "error='$($json.error)'"
    Check 'a_refusal_is_not_a_receipt' (-not (Test-Path (Join-Path $scratch ('runtime\receipts\' + $requestId + '.json')))) `
        'a refused request must not leave a receipt behind'

    Write-Output ("draft failure path: {0} passed, 0 failed (cause reported: {1})" -f $passed.Count, $json.error)
    Write-Output 'No messages were sent.'
} finally {
    Remove-Item -Recurse -Force $scratch -ErrorAction SilentlyContinue
}
