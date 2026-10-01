[CmdletBinding()]
param(
    [string]$DshDataHome = $env:A2A_DSH_DATA_HOME,
    [string]$Workspace = '',
    [string]$DshWebOrigin = 'http://127.0.0.1:3080',
    [string[]]$DshBrowserProcesses = @('msedge', 'chrome'),
    [string]$ClaudeHost = 'claude.ai',
    [string]$DshPageTitlePattern = 'DeepSeek Harness',
    [switch]$Force
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

$product = Split-Path $PSScriptRoot -Parent
$runtime = Join-Path $product 'runtime'
New-Item -ItemType Directory -Force $runtime | Out-Null

if ([string]::IsNullOrWhiteSpace($DshDataHome)) {
    foreach ($proc in @(Get-CimInstance Win32_Process -Filter "Name='node.exe'" -ErrorAction SilentlyContinue)) {
        $cmd = [string]$proc.CommandLine
        if ($cmd -match '(?i)([A-Z]:\\.+?)\\runtime\\node_modules\\@deepseek-ai\\dsh\\lib\\bin\.js') {
            $candidate = Join-Path $Matches[1] 'data'
            if (Test-Path (Join-Path $candidate 'storages\session_projcache\sessions')) {
                $DshDataHome = $candidate
                break
            }
        }
    }
}
if ([string]::IsNullOrWhiteSpace($DshDataHome)) {
    throw 'DSH data directory was not detected. Re-run with -DshDataHome <path> or set A2A_DSH_DATA_HOME.'
}
$DshDataHome = [IO.Path]::GetFullPath($DshDataHome)
if (-not (Test-Path (Join-Path $DshDataHome 'storages\session_projcache\sessions'))) {
    throw "Not a supported DSH data directory: $DshDataHome"
}
if ($Workspace) {
    $Workspace = [IO.Path]::GetFullPath($Workspace)
    if (-not (Test-Path $Workspace)) { throw "Workspace does not exist: $Workspace" }
}

# Optional detection of the running DSH web UI. The DSH web process carries its
# origin on the command line, for example:
#   "...\@deepseek-ai\dsh\lib\bin.js web --host 127.0.0.1 --port 3080"
# Detection is best effort: on any failure the documented default is used.
$detectedOrigin = $null
try {
    foreach ($proc in @(Get-CimInstance Win32_Process -Filter "Name='node.exe'" -ErrorAction SilentlyContinue)) {
        $cmd = [string]$proc.CommandLine
        if ($cmd -notmatch '(?i)\\@deepseek-ai\\dsh\\lib\\bin\.js\s+web\b') { continue }
        if ($cmd -notmatch '(?i)--port[=\s]+(\d{1,5})') { continue }
        $detectedPort = [int]$Matches[1]
        $detectedHost = if ($cmd -match '(?i)--host[=\s]+([^\s"]+)') { $Matches[1] } else { '127.0.0.1' }
        if ($detectedHost -in @('0.0.0.0', '::', '[::]')) { $detectedHost = '127.0.0.1' }
        if ($detectedPort -ge 1 -and $detectedPort -le 65535) {
            $detectedOrigin = "http://${detectedHost}:$detectedPort"
            break
        }
    }
} catch { $detectedOrigin = $null }

# An explicitly supplied -DshWebOrigin always wins over detection.
# NOTE: $PSBoundParameters keys carry no leading dash.
$webOriginText = $(if ($PSBoundParameters.ContainsKey('DshWebOrigin') -or -not $detectedOrigin) { $DshWebOrigin } else { $detectedOrigin })
$origin = $null
if (-not [Uri]::TryCreate($webOriginText, [UriKind]::Absolute, [ref]$origin) -or $origin.Scheme -notin @('http', 'https')) {
    throw "Invalid DSH web origin: $webOriginText (expected http:// or https://)"
}
$dshWebOrigin = '{0}://{1}:{2}' -f $origin.Scheme.ToLowerInvariant(), $origin.Host.ToLowerInvariant(), $origin.Port

# The page title is only readable when the DSH web UI answers without asking for
# authentication; a 401/403 answer still proves that the server is there.
$pageTitlePattern = $DshPageTitlePattern
$webDetected = $false
try {
    $probe = Invoke-WebRequest -Uri $dshWebOrigin -UseBasicParsing -TimeoutSec 2
    $webDetected = $true
    if (-not $PSBoundParameters.ContainsKey('DshPageTitlePattern') -and
        ([string]$probe.Content) -match '(?is)<title[^>]*>(.*?)</title>') {
        $detectedTitle = ($Matches[1] -replace '\s+', ' ').Trim()
        if ($detectedTitle) { $pageTitlePattern = $detectedTitle }
    }
} catch {
    if ($null -ne $_.Exception.Response) { $webDetected = $true }
}

$browsers = @($DshBrowserProcesses | ForEach-Object { ([string]$_).Trim().ToLowerInvariant() } | Where-Object { $_ })
if (-not $browsers.Count) { $browsers = @('msedge', 'chrome') }
if (-not $ClaudeHost.Trim()) { $ClaudeHost = 'claude.ai' }
$ClaudeHost = $ClaudeHost.Trim().ToLowerInvariant()
if (-not ([string]$pageTitlePattern).Trim()) { $pageTitlePattern = 'DeepSeek Harness' }
$pageTitlePattern = ([string]$pageTitlePattern).Trim()

$configPath = Join-Path $runtime 'config.json'
if ((Test-Path $configPath) -and -not $Force) {
    throw 'runtime\config.json already exists. Use -Force only if you intend to replace local settings.'
}
$config = [ordered]@{
    enabled = $false
    poll_seconds = 60
    dispatch_delay_seconds = 10
    dsh_data_home = $DshDataHome
    dsh_web_origin = $dshWebOrigin
    dsh_browser_processes = @($browsers)
    workspace = $Workspace
    claude_host = $ClaudeHost
    dsh_page_title_pattern = $pageTitlePattern
}
$json = $config | ConvertTo-Json -Depth 6
[IO.File]::WriteAllText($configPath, $json, [Text.UTF8Encoding]::new($false))

$templatePath = Join-Path $runtime 'message-templates.json'
if (-not (Test-Path $templatePath)) {
    Copy-Item (Join-Path $product 'examples\message-templates.json') $templatePath
}

$bindingPath = Join-Path $runtime 'bindings.json'
if (-not (Test-Path $bindingPath)) {
    $placeholder = [ordered]@{
        claude_session = 'cse_unconfigured'
        claude_title = 'Claude Desktop'
        claude_window = 'Claude Desktop'
        dsh_session = 'session-unconfigured'
    }
    [IO.File]::WriteAllText($bindingPath, ($placeholder | ConvertTo-Json), [Text.UTF8Encoding]::new($false))
}


[pscustomobject]@{
    ok = $true
    product_root = $product
    dsh_data_home = $DshDataHome
    workspace_check = $(if ($Workspace) { $Workspace } else { 'disabled' })
    dsh_web_origin = $config.dsh_web_origin
    dsh_web_detected = $webDetected
    dsh_browser_processes = @($browsers)
    claude_host = $config.claude_host
    dsh_page_title_pattern = $config.dsh_page_title_pattern
    next_step = 'Start A2AHandoff and open Binding Configuration to select DSH and enter Claude cse_ Session ID.'
} | ConvertTo-Json -Depth 5
