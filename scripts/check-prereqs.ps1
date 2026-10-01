[CmdletBinding()]
param([switch]$Development)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

$product = Split-Path $PSScriptRoot -Parent
$checks = [Collections.Generic.List[object]]::new()
function Add-Check($Name, $Ok, $Detail) {
    $script:checks.Add([pscustomobject]@{ name = $Name; ok = [bool]$Ok; detail = [string]$Detail })
}
function Get-Setting($Object, $Name, $Default = $null) {
    if ($null -ne $Object -and $null -ne $Object.PSObject.Properties[$Name]) { return $Object.$Name }
    return $Default
}

Add-Check 'Windows' ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) ([Environment]::OSVersion.VersionString)
Add-Check 'PowerShell' ($PSVersionTable.PSVersion.Major -ge 5) $PSVersionTable.PSVersion.ToString()

$node = Get-Command node.exe -ErrorAction SilentlyContinue
if ($node) {
    $nodeVersion = (& node.exe --version 2>$null)
    & node.exe -e "const z=require('node:zlib');process.exit(typeof z.zstdDecompressSync==='function'?0:3)"
    Add-Check 'Node zstd support' ($LASTEXITCODE -eq 0) "$nodeVersion · $($node.Source)"
} else {
    Add-Check 'Node zstd support' $false 'node.exe not found on PATH'
}

$configPath = Join-Path $product 'runtime\config.json'
$config = $null
if (Test-Path $configPath) {
    try { $config = Get-Content $configPath -Raw -Encoding UTF8 | ConvertFrom-Json } catch {}
}
Add-Check 'Local config' ($null -ne $config) $configPath
if ($null -ne $config) {
    $data = [string](Get-Setting $config 'dsh_data_home' '')
    $sessions = if ($data) { Join-Path $data 'storages\session_projcache\sessions' } else { '' }
    Add-Check 'DSH data directory' ($sessions -and (Test-Path $sessions)) $data

    $origin = [string](Get-Setting $config 'dsh_web_origin' 'http://127.0.0.1:3080')
    try {
        $uri = [Uri]$origin
        $tcp = Test-NetConnection -ComputerName $uri.Host -Port $uri.Port -WarningAction SilentlyContinue
        Add-Check 'DSH web endpoint' $tcp.TcpTestSucceeded $origin
    } catch {
        Add-Check 'DSH web endpoint' $false $origin
    }

    # Advanced settings are reported with their documented fallbacks; they are
    # informational and never fail the run.
    $claudeHost = ([string](Get-Setting $config 'claude_host' 'claude.ai')).Trim()
    if (-not $claudeHost) { $claudeHost = 'claude.ai' }
    Add-Check 'Claude host' $true $claudeHost

    $titlePattern = [string](Get-Setting $config 'dsh_page_title_pattern' 'DeepSeek Harness')
    if (-not $titlePattern.Trim()) { $titlePattern = 'DeepSeek Harness' }
    Add-Check 'DSH page title pattern' $true $titlePattern

    # Read the raw property: Get-Setting unrolls a one-element JSON array.
    $browserProperty = $config.PSObject.Properties['dsh_browser_processes']
    $browserNames = @()
    if ($null -ne $browserProperty -and $browserProperty.Value -is [array]) {
        $browserNames = @($browserProperty.Value | ForEach-Object { ([string]$_).Trim().ToLowerInvariant() } | Where-Object { $_ })
    }
    if (-not $browserNames.Count) { $browserNames = @('msedge', 'chrome') }
    Add-Check 'DSH browser processes' $true ($browserNames -join ', ')
}

$claude = @(Get-Process claude -ErrorAction SilentlyContinue)
Add-Check 'Claude Desktop running' ($claude.Count -gt 0) $(if ($claude.Count) { $claude[0].Path } else { 'not running' })

if ($Development) {
    $cargo = Get-Command cargo.exe -ErrorAction SilentlyContinue
    $rustc = Get-Command rustc.exe -ErrorAction SilentlyContinue
    Add-Check 'Cargo' ($null -ne $cargo) $(if ($cargo) { & cargo --version } else { 'not found' })
    Add-Check 'Rust compiler' ($null -ne $rustc) $(if ($rustc) { & rustc --version } else { 'not found' })
}

$checks | Format-Table -AutoSize
$failed = @($checks | Where-Object { -not $_.ok })
if ($failed.Count) {
    Write-Error "$($failed.Count) prerequisite check(s) failed."
    exit 1
}
Write-Output 'All requested prerequisite checks passed. No messages were sent.'
