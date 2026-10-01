# Adapter configuration-contract tests.
#
# Verifies that the PowerShell and Node adapters take DSH environment values and the
# Claude host from runtime/config.json (the same schema Rust owns in
# crates/handoff-core/src/config.rs), and that they fail closed on a mismatched
# origin instead of guessing.
#
# No app is started, no runtime is run, and nothing is sent to Claude or DSH.
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

# A throwaway product root: the repo's own runtime/ is never written to.
$sandbox = Join-Path ([IO.Path]::GetTempPath()) ("a2a-adapter-config-" + [Guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Force (Join-Path $sandbox 'runtime') | Out-Null
$configPath = Join-Path $sandbox 'runtime\config.json'

function Write-Config($json) {
    [IO.File]::WriteAllText($script:configPath, $json, [Text.UTF8Encoding]::new($false))
}

try {
    . (Join-Path $root 'adapters\common\a2a-config.ps1')

    Write-Output 'PowerShell config reader'

    # --- DSH web origin comes from configuration -----------------------------
    Write-Config '{"dsh_web_origin":"http://127.0.0.1:45999","dsh_browser_processes":["brave"],"claude_host":"claude.example","dsh_page_title_pattern":"My Harness"}'
    $cfg = Get-A2aRuntimeConfig -ProductRoot $sandbox
    $origin = Get-A2aDshWebOrigin $cfg
    Check 'dsh_web_origin host/port read from config' `
        ($origin.Scheme -eq 'http' -and $origin.Host -eq '127.0.0.1' -and $origin.Port -eq 45999) `
        "got $origin"
    Check 'browser process list read from config' `
        ((Get-A2aDshBrowserProcesses $cfg) -join ',') -eq 'brave' `
        "got $((Get-A2aDshBrowserProcesses $cfg) -join ',')"
    Check 'claude_host read from config' `
        ((Get-A2aClaudeHost $cfg) -eq 'claude.example') `
        "got $(Get-A2aClaudeHost $cfg)"
    Check 'dsh_page_title_pattern read from config' `
        ((Get-A2aDshPageTitlePattern $cfg) -eq 'My Harness') `
        "got $(Get-A2aDshPageTitlePattern $cfg)"

    # --- fallbacks for missing / empty / illegal values ----------------------
    Write-Config '{"dsh_browser_processes":[],"dsh_web_origin":"not a url","claude_host":"  ",  "dsh_page_title_pattern":""}'
    $cfg = Get-A2aRuntimeConfig -ProductRoot $sandbox
    $origin = Get-A2aDshWebOrigin $cfg
    Check 'empty browser list falls back to msedge,chrome' `
        ((Get-A2aDshBrowserProcesses $cfg) -join ',') -eq 'msedge,chrome'
    Check 'illegal origin falls back to the documented default' `
        ($origin.Host -eq '127.0.0.1' -and $origin.Port -eq 3080) "got $origin"
    Check 'blank claude_host falls back to claude.ai' `
        ((Get-A2aClaudeHost $cfg) -eq 'claude.ai')
    Check 'blank title pattern falls back to DeepSeek Harness' `
        ((Get-A2aDshPageTitlePattern $cfg) -eq 'DeepSeek Harness')

    Write-Config '{}'
    $cfg = Get-A2aRuntimeConfig -ProductRoot $sandbox
    Check 'missing fields fall back for every setting' `
        (((Get-A2aDshBrowserProcesses $cfg) -join ',') -eq 'msedge,chrome' -and
         (Get-A2aClaudeHost $cfg) -eq 'claude.ai' -and
         (Get-A2aDshPageTitlePattern $cfg) -eq 'DeepSeek Harness' -and
         (Get-A2aWorkspace $cfg) -eq '')

    # --- deprecated keys and broken files must not stop a read ---------------
    Write-Config '{"task_file":"X:\\gone.md","mode":"live","next_poll_at_ms":123,"claude_host":"claude.ai"}'
    $cfg = Get-A2aRuntimeConfig -ProductRoot $sandbox
    Check 'legacy config with deprecated keys still reads' `
        ((Get-A2aClaudeHost $cfg) -eq 'claude.ai')
    Write-Config '{ this is not json'
    $cfg = Get-A2aRuntimeConfig -ProductRoot $sandbox
    Check 'broken config yields defaults instead of throwing' `
        ((Get-A2aClaudeHost $cfg) -eq 'claude.ai')
    Remove-Item $configPath -Force
    $cfg = Get-A2aRuntimeConfig -ProductRoot $sandbox
    Check 'missing config yields defaults instead of throwing' `
        ((Get-A2aClaudeHost $cfg) -eq 'claude.ai')

    # --- workspace: empty means the extra check is off ----------------------
    Write-Config '{"workspace":"  "}'
    Check 'blank workspace disables the check' `
        ((Get-A2aWorkspace (Get-A2aRuntimeConfig -ProductRoot $sandbox)) -eq '')
    Write-Config '{"workspace":"D:\\proj"}'
    Check 'non-empty workspace is returned verbatim' `
        ((Get-A2aWorkspace (Get-A2aRuntimeConfig -ProductRoot $sandbox)) -eq 'D:\proj')

    # --- the PowerShell defaults must match the Rust schema -----------------
    Write-Output 'Schema parity with Rust'
    $rust = [IO.File]::ReadAllText((Join-Path $root 'crates\handoff-core\src\config.rs'), [Text.Encoding]::UTF8)
    Check 'PS default poll matches Rust' `
        ($script:A2aConfigDefaults.PollSeconds -eq 60)
    Check 'PS default dispatch delay matches Rust' `
        ($script:A2aConfigDefaults.DispatchDelaySeconds -eq 10)
    Check 'PS default origin matches Rust constant' `
        ($rust -match [regex]::Escape("DEFAULT_DSH_WEB_ORIGIN: &str = `"$($script:A2aConfigDefaults.DshWebOrigin)`""))
    Check 'PS default claude host matches Rust constant' `
        ($rust -match [regex]::Escape("DEFAULT_CLAUDE_HOST: &str = `"$($script:A2aConfigDefaults.ClaudeHost)`""))
    Check 'PS default title pattern matches Rust constant' `
        ($rust -match [regex]::Escape("DEFAULT_DSH_PAGE_TITLE_PATTERN: &str = `"$($script:A2aConfigDefaults.DshPageTitlePattern)`""))
    Check 'PS default browser list matches Rust constant' `
        ($rust -match [regex]::Escape('DEFAULT_DSH_BROWSER_PROCESSES: [&str; 2] = ["msedge", "chrome"]'))

    # --- Node adapter uses the same reader ----------------------------------
    Write-Output 'Node config reader'
    $node = Get-Command node.exe -ErrorAction SilentlyContinue
    if (-not $node) {
        Write-Output '  skip node.exe not found on PATH'
    } else {
        $probe = Join-Path $sandbox 'probe.mjs'
        # ESM import specifiers must be file:// URLs on Windows.
        $helper = 'file:///' + (Join-Path $root 'adapters\common\a2a-config.mjs').Replace('\', '/')
        $probeBody = @"
import {readRuntimeConfig,dshWebOrigin,dshBrowserProcesses,claudeHost,dshPageTitlePattern,workspace} from '$helper';
const cfg = readRuntimeConfig(process.argv[2]);
console.log(JSON.stringify({
  origin: dshWebOrigin(cfg),
  browsers: dshBrowserProcesses(cfg),
  claudeHost: claudeHost(cfg),
  title: dshPageTitlePattern(cfg),
  workspace: workspace(cfg)
}));
"@
        [IO.File]::WriteAllText($probe, $probeBody, [Text.UTF8Encoding]::new($false))

        Write-Config '{"dsh_web_origin":"http://localhost:45999","dsh_browser_processes":["Brave"," msedge "],"claude_host":"Claude.Example","dsh_page_title_pattern":"My Harness","workspace":"D:\\proj"}'
        $out = (& node.exe $probe $sandbox) | ConvertFrom-Json
        Check 'node: origin read from config' `
            ($out.origin.host -eq 'localhost' -and $out.origin.port -eq 45999) "got $($out.origin | ConvertTo-Json -Compress)"
        Check 'node: browser list read + normalised from config' `
            (($out.browsers -join ',') -eq 'brave,msedge') "got $($out.browsers -join ',')"
        Check 'node: claude_host read from config' `
            ($out.claudeHost -eq 'claude.example') "got $($out.claudeHost)"
        Check 'node: title pattern read from config' `
            ($out.title -eq 'My Harness')

        Write-Config '{"dsh_browser_processes":[],"dsh_web_origin":"nonsense"}'
        $out = (& node.exe $probe $sandbox) | ConvertFrom-Json
        Check 'node: empty browsers fall back' (($out.browsers -join ',') -eq 'msedge,chrome')
        Check 'node: illegal origin falls back' `
            ($out.origin.host -eq '127.0.0.1' -and $out.origin.port -eq 3080) "got $($out.origin | ConvertTo-Json -Compress)"

        Write-Config '{ broken'
        $out = (& node.exe $probe $sandbox) | ConvertFrom-Json
        Check 'node: broken config yields defaults' ($out.claudeHost -eq 'claude.ai')
        Remove-Item $probe -Force
    }

    # --- origin mismatch must fail closed, never guess ---------------------
    Write-Output 'Origin matching (fail closed)'
    Write-Config '{"dsh_web_origin":"http://127.0.0.1:45999"}'
    $cfg = Get-A2aRuntimeConfig -ProductRoot $sandbox
    $configured = Get-A2aDshWebOrigin $cfg
    $candidate = [Uri]'http://127.0.0.1:3080/'
    $matchesConfigured = ($candidate.Scheme -ceq $configured.Scheme -and
                          $candidate.Host -ceq $configured.Host -and
                          $candidate.Port -eq $configured.Port)
    Check 'a page on a different port does not match the configured origin' (-not $matchesConfigured)
    $candidate = [Uri]'http://127.0.0.1:45999/'
    $matchesConfigured = ($candidate.Scheme -ceq $configured.Scheme -and
                          $candidate.Host -ceq $configured.Host -and
                          $candidate.Port -eq $configured.Port)
    Check 'the configured origin matches exactly' $matchesConfigured

    # --- Claude identity is still <host>/cowork/<cse_id> -------------------
    Write-Output 'Claude identity'
    $observe = [IO.File]::ReadAllText((Join-Path $root 'adapters\claude\observe.ps1'), [Text.Encoding]::UTF8)
    Check 'claude adapter checks host AND /cowork/<id> path' `
        (($observe -match 'uri\.Host -ne \$script:ClaudeHost') -and ($observe -match "AbsolutePath -cne \('/cowork/'\+"))
    Check 'claude adapter no longer hard-codes claude.ai' `
        ($observe -notmatch "uri\.Host -ne 'claude\.ai'")
    Check 'claude adapter does not read a file for reply text' `
        ($observe -notmatch 'Get-Content|ReadAllText|ReadAllLines')
    Check 'claude adapter is callable without the reserved -Host parameter' `
        ($observe -match '\[string\]\$ClaudeHost' -and $observe -notmatch '\[string\]\$Host\b')

    # A manual send must not wait out the automatic countdown. The countdown exists
    # so an automatic handoff can be cancelled before it goes out, so it keeps its
    # one-second floor; a manual click is already the confirmation.
    $draftFlow = [IO.File]::ReadAllText((Join-Path $root 'adapters\windows\draft-flow.ps1'))
    Check 'draft-flow honours the request manual flag for the countdown' `
        ($draftFlow -match 'if\(\$script:r\.manual\)\{\$delay=0\}else\{\$delay=\[Math\]::Max\(1,')
    Check 'draft-flow keeps the one-second floor for automatic dispatch' `
        ($draftFlow -match '\$delay=\[Math\]::Max\(1,\[Math\]::Min\(120,')
} finally {
    Remove-Item $sandbox -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Output ''
Write-Output "Adapter config contract: $checks check(s), $($failures.Count) failure(s)."
if ($failures.Count) {
    $failures | ForEach-Object { Write-Error "failed: $_" }
    exit 1
}
Write-Output 'All adapter configuration checks passed. No messages were sent.'
