<#
.SYNOPSIS
  Prepare the pi-harness bundle the installer ships: a fixed Node runtime plus
  harness/pi with production-only node_modules, both under dist\.

.DESCRIPTION
  The installed app runs the AI analyze chain through Node (harness/pi/analyze.mjs)
  and looks for the runtime BESIDE its own exe:
      <app>\node\node.exe          (AUTOSHADE_NODE overrides both)
      <app>\harness\pi\analyze.mjs
  This script fills dist\node and dist\harness from the repo so
  `ISCC /DAppVersion=x.y.z installer\autoshade.iss` can package them.

  The Node version is PINNED and its SHA-256 is verified against nodejs.org's
  SHASUMS256.txt before the archive is unpacked.

  -SkipNode tests the harness copy + npm install without the network download.

.NOTES
  Run from anywhere; paths resolve from this script's location:
      <repo>\installer\prepare-harness.ps1
  Exit code 0 = dist\harness\ (and dist\node\ unless -SkipNode) is ready.
#>
[CmdletBinding()]
param(
    [string]$NodeVersion = "22.14.0",
    [string]$RepoRoot = "",
    [switch]$SkipNode
)

$ErrorActionPreference = "Stop"

# $PSScriptRoot is not populated while parameter defaults evaluate under
# Windows PowerShell 5.1; resolve the script directory in the body instead.
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $RepoRoot) {
    $RepoRoot = (Resolve-Path (Join-Path $scriptDir "..\..\..")).Path
}
$dist = Join-Path $scriptDir "..\dist"
$harnessSrc = Join-Path $RepoRoot "harness\pi"
if (-not (Test-Path (Join-Path $harnessSrc "analyze.mjs"))) {
    throw "harness source not found: $harnessSrc (pass -RepoRoot to the photo repo)"
}
$npm = Get-Command npm.cmd -ErrorAction SilentlyContinue
if (-not $npm) { throw "npm.cmd not found on PATH" }

# 1. Copy the harness sources WITHOUT the dev checkout's node_modules, sessions
#    or render artifacts.
$dest = Join-Path $dist "harness\pi"
if (Test-Path $dest) { Remove-Item -Recurse -Force $dest }
New-Item -ItemType Directory -Force -Path $dest | Out-Null
Copy-Item (Join-Path $harnessSrc "*.mjs") $dest
Copy-Item (Join-Path $harnessSrc "package.json"), (Join-Path $harnessSrc "package-lock.json") $dest
if (Test-Path (Join-Path $harnessSrc "prompts")) {
    Copy-Item -Recurse (Join-Path $harnessSrc "prompts") (Join-Path $dest "prompts")
}

# 2. Production dependencies only, from the lockfile.
Push-Location $dest
try {
    & $npm.Source ci --omit=dev --no-audit --no-fund
    if ($LASTEXITCODE -ne 0) { throw "npm ci failed in $dest" }
} finally {
    Pop-Location
}
$pkg = (Get-ChildItem (Join-Path $dest "node_modules") -Directory | Measure-Object).Count
Write-Host "harness: $dest ($pkg package folders)"

# 3. Fixed Node runtime (node.exe is self-contained on win-x64).
if (-not $SkipNode) {
    $zip = Join-Path $env:TEMP "node-v$NodeVersion-win-x64.zip"
    $url = "https://nodejs.org/dist/v$NodeVersion/node-v$NodeVersion-win-x64.zip"
    Write-Host "node: downloading $url"
    Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing
    $sums = (Invoke-WebRequest -Uri "https://nodejs.org/dist/v$NodeVersion/SHASUMS256.txt" -UseBasicParsing).Content
    $line = ($sums -split "`n") | Where-Object { $_ -match "node-v$NodeVersion-win-x64\.zip$" } | Select-Object -First 1
    if (-not $line) { throw "no SHA-256 line for node-v$NodeVersion-win-x64.zip" }
    $want = ($line.Trim() -split "\s+")[0].ToUpperInvariant()
    $got = (Get-FileHash -Algorithm SHA256 -LiteralPath $zip).Hash.ToUpperInvariant()
    if ($got -ne $want) { throw "node archive sha256 mismatch: got $got want $want" }

    $tmp = Join-Path $dist "node-tmp"
    if (Test-Path $tmp) { Remove-Item -Recurse -Force $tmp }
    Expand-Archive -LiteralPath $zip -DestinationPath $tmp -Force
    $src = Join-Path $tmp "node-v$NodeVersion-win-x64"
    $nodeDir = Join-Path $dist "node"
    if (Test-Path $nodeDir) { Remove-Item -Recurse -Force $nodeDir }
    New-Item -ItemType Directory -Force -Path $nodeDir | Out-Null
    Copy-Item (Join-Path $src "node.exe") $nodeDir
    Copy-Item (Join-Path $src "LICENSE") $nodeDir
    Remove-Item -Recurse -Force $tmp, $zip
    Write-Host "node: $nodeDir\node.exe"
}

Write-Host "done: $dist"
