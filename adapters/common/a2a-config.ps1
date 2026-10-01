# Shared reader for runtime/config.json.
#
# Rust (`crates/handoff-core/src/config.rs`) owns the schema. This file must never
# invent different defaults: the values below mirror the Rust constants, and every
# PowerShell adapter reads configuration through these helpers.
#
# Dot-source it:  . (Join-Path $PSScriptRoot '..\common\a2a-config.ps1')

$script:A2aConfigDefaults = [ordered]@{
    PollSeconds          = 60
    DispatchDelaySeconds = 10
    DshWebOrigin         = 'http://127.0.0.1:3080'
    DshBrowserProcesses  = @('msedge', 'chrome')
    ClaudeHost           = 'claude.ai'
    DshPageTitlePattern  = 'DeepSeek Harness'
}

# Reads runtime/config.json. A missing or broken file yields the defaults, matching
# the Rust loader: configuration problems must never stop the app from starting.
function Get-A2aRuntimeConfig {
    param([Parameter(Mandatory = $true)][string]$ProductRoot)
    $path = Join-Path $ProductRoot 'runtime\config.json'
    if (-not (Test-Path -LiteralPath $path)) { return [pscustomobject]$script:A2aConfigDefaults }
    try {
        $raw = [IO.File]::ReadAllText($path, [Text.Encoding]::UTF8)
        $raw = $raw -replace '^\uFEFF', ''
        $cfg = $raw | ConvertFrom-Json
    } catch {
        return [pscustomobject]$script:A2aConfigDefaults
    }
    if ($null -eq $cfg) { return [pscustomobject]$script:A2aConfigDefaults }
    return $cfg
}

function Get-A2aConfigField {
    param($Config, [Parameter(Mandatory = $true)][string]$Name, $Default = $null)
    if ($null -ne $Config -and $null -ne $Config.PSObject.Properties[$Name]) {
        $value = $Config.$Name
        if ($null -ne $value) { return $value }
    }
    return $Default
}

# Resolved values. Each one applies the documented fallback so callers never have
# to repeat it.

function Get-A2aDshDataHome {
    param($Config)
    $value = [string](Get-A2aConfigField $Config 'dsh_data_home' '')
    return $value.Trim()
}

function Get-A2aDshWebOrigin {
    param($Config)
    # Accepts "http://host:port" (and normalizes it); an unusable value falls back.
    $text = ([string](Get-A2aConfigField $Config 'dsh_web_origin' $script:A2aConfigDefaults.DshWebOrigin)).Trim()
    $uri = $null
    if ([Uri]::TryCreate($text, [UriKind]::Absolute, [ref]$uri) -and $uri.Scheme -in @('http', 'https')) {
        return $uri
    }
    return [Uri]$script:A2aConfigDefaults.DshWebOrigin
}

function Get-A2aDshBrowserProcesses {
    param($Config)
    $configured = @(Get-A2aConfigField $Config 'dsh_browser_processes' $script:A2aConfigDefaults.DshBrowserProcesses)
    $names = @($configured | ForEach-Object { ([string]$_).Trim().ToLowerInvariant() } | Where-Object { $_ })
    if ($names.Count -eq 0) { return $script:A2aConfigDefaults.DshBrowserProcesses }
    return $names
}

function Get-A2aClaudeHost {
    param($Config)
    $value = ([string](Get-A2aConfigField $Config 'claude_host' $script:A2aConfigDefaults.ClaudeHost)).Trim()
    if (-not $value) { return $script:A2aConfigDefaults.ClaudeHost }
    return $value.ToLowerInvariant()
}

function Get-A2aDshPageTitlePattern {
    param($Config)
    $value = [string](Get-A2aConfigField $Config 'dsh_page_title_pattern' $script:A2aConfigDefaults.DshPageTitlePattern)
    if (-not $value.Trim()) { return $script:A2aConfigDefaults.DshPageTitlePattern }
    return $value
}

function Get-A2aWorkspace {
    param($Config)
    $value = [string](Get-A2aConfigField $Config 'workspace' '')
    return $value.Trim()
}

function Get-A2aAutoEnabled {
    param($Config)
    return [bool](Get-A2aConfigField $Config 'enabled' $false)
}
