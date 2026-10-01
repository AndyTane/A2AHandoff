# Assembles the download a user unzips: the two executables, the adapters they drive, the
# scripts they run for first-time configuration, the examples bootstrap copies from, and the
# user-facing docs. tests/, crates/, the internal notes and the build scripts stay out.
#
# The executables are built separately, because the running app holds its own .exe open and a
# plain `cargo build --release` cannot replace it:
#
#   cargo build --release --locked --target-dir target/draft-first
#   powershell -ExecutionPolicy Bypass -File scripts\package-release.ps1
#
# Output: %TEMP%\a2a-release\A2AHandoff-<version>-windows-x64.zip and a .sha256 beside it.
# Nothing is committed: .gitignore excludes *.exe, and the zip is written outside the tree.
[CmdletBinding()]
param(
    [string]$Version = '',
    [string]$TargetDir = '',
    [string]$OutDir = ''
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

$root = Split-Path $PSScriptRoot -Parent
if (-not $Version) {
    # The UI crate carries the product version; the release tag matches it.
    $toml = [IO.File]::ReadAllText((Join-Path $root 'crates\a2a-ui-windows\Cargo.toml'), [Text.Encoding]::UTF8)
    if ($toml -notmatch '(?m)^version\s*=\s*"([^"]+)"') {
        throw 'No version field in crates/a2a-ui-windows/Cargo.toml'
    }
    $Version = $Matches[1]
}
if (-not $TargetDir) { $TargetDir = Join-Path $root 'target\draft-first\release' }
if (-not $OutDir) { $OutDir = Join-Path $env:TEMP 'a2a-release' }

$name = "A2AHandoff-$Version-windows-x64"
$stage = Join-Path $OutDir $name
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force $stage | Out-Null

# 1. The executables. handoff-runtime.exe is spawned from the same directory as the UI, so a
#    package with only one of them starts and then reports nothing.
foreach ($exe in @('A2AHandoff.exe', 'handoff-runtime.exe')) {
    $from = Join-Path $TargetDir $exe
    if (-not (Test-Path $from)) {
        throw "missing $from - build with: cargo build --release --locked --target-dir target/draft-first"
    }
    Copy-Item $from $stage
}

# 2. Everything the runtime drives by relative path. The adapters are the delivery machinery:
#    a package without them opens a window and can never read or send anything.
Copy-Item (Join-Path $root 'adapters') $stage -Recurse
Copy-Item (Join-Path $root 'examples') $stage -Recurse
New-Item -ItemType Directory -Force (Join-Path $stage 'scripts') | Out-Null
foreach ($script in @('bootstrap.ps1', 'dsh-detect.ps1', 'check-prereqs.ps1')) {
    Copy-Item (Join-Path $root "scripts\$script") (Join-Path $stage 'scripts')
}

# 3. An empty runtime/, so the first run has somewhere to write. The .gitkeep keeps the
#    directory in the archive: Compress-Archive drops empty directories.
New-Item -ItemType Directory -Force (Join-Path $stage 'runtime') | Out-Null
Copy-Item (Join-Path $root 'runtime\.gitkeep') (Join-Path $stage 'runtime')

# 4. The docs a user needs; the dated repair notes, the QA audit and the readiness checklist
#    are about building the project, not using it.
New-Item -ItemType Directory -Force (Join-Path $stage 'docs') | Out-Null
foreach ($doc in @('CONFIGURATION.md', 'FEATURES.md', 'A2AHANDOFF_UI.md', 'MESSAGE_TEMPLATES.md',
        'MANUAL_INTERVENTION.md', 'BEHAVIOR_CONTRACT.md')) {
    Copy-Item (Join-Path $root "docs\$doc") (Join-Path $stage 'docs')
}
Copy-Item (Join-Path $root 'LICENSE') $stage
Copy-Item (Join-Path $root 'README.md') $stage

# 4b. What the user's agent reads. AGENTS.md is the install/repair procedure, and the skill is
# the same steps in the format a DSH or Claude agent catalog loads - so the user can hand the
# folder to their agent instead of explaining the tool to it.
Copy-Item (Join-Path $root 'AGENTS.md') $stage
Copy-Item (Join-Path $root 'skills') $stage -Recurse

# 5. The three steps, in the archive root where they cannot be missed.
$firstRun = @"
A2AHandoff $Version - Windows x64

Requirements: Claude Desktop, DeepSeek Harness (web UI running), Node.js with zstd support
(check with scripts\check-prereqs.ps1), Windows PowerShell 5.1, Edge or Chrome.
Do not move the .exe files out of this folder: they load ..\adapters from beside them.

1. Configure this machine (creates runtime\config.json, detects your DSH data directory):

     powershell -ExecutionPolicy Bypass -File .\scripts\bootstrap.ps1

2. Start the app:

     .\A2AHandoff.exe --product-root .

   Double-clicking it works too: it finds this folder and writes runtime\config.json itself.

3. In the window, open "Binding Configuration": select the DSH session and paste the
   Claude Cowork cse_... Session ID. Then press "监听" to start watching.

Automatic handoff is off until you turn on "自动交接". Nothing is sent without a binding.

Your agent can do steps 1 and 2 and walk you through step 3: point it at AGENTS.md, or copy
skills\a2a-handoff-setup into its skills folder so it knows this tool by itself.
"@
Set-Content (Join-Path $stage 'START-HERE.txt') $firstRun -Encoding UTF8

# 6. The archive, and the hash a downloader can check.
$zip = Join-Path $OutDir "$name.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path $stage -DestinationPath $zip -CompressionLevel Optimal
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash
"$hash  $name.zip" | Set-Content (Join-Path $OutDir "$name.zip.sha256") -Encoding ASCII

$files = @(Get-ChildItem $stage -Recurse -File)
Write-Output ("packaged {0}: {1} files, {2:N1} KB unpacked, {3:N1} KB zipped" -f `
        $name, $files.Count, (($files | Measure-Object Length -Sum).Sum / 1KB), ((Get-Item $zip).Length / 1KB))
Write-Output "  $zip"
Write-Output "  $zip.sha256"
Write-Output "  sha256 $hash"
