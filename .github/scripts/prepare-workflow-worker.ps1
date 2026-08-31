param(
  [string]$WorkerRepository = "https://github.com/yanmengli123/rice-endosperm-workflow.git",
  [string]$WorkerCommit = "0b06c20bac6eae60b65078a85d4be1eb480537cd",
  [string]$SourceDirectory = "",
  [switch]$AllowDirtySource
)

$ErrorActionPreference = "Stop"
$desktopRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$resourceRoot = Join-Path $desktopRoot "src-tauri\resources"
$noticeRoot = Join-Path $resourceRoot "workflow"
$engineResourceRoot = Join-Path $resourceRoot "workflow-engine"
$tempParent = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [System.IO.Path]::GetTempPath() }
New-Item -ItemType Directory -Force -Path $noticeRoot | Out-Null
New-Item -ItemType Directory -Force -Path $engineResourceRoot | Out-Null
$temporaryCheckout = $null
try {
$checkout = if ([string]::IsNullOrWhiteSpace($SourceDirectory)) {
  $temporaryCheckout = Join-Path $tempParent ("rice-workflow-worker-" + [guid]::NewGuid().ToString("N"))
  New-Item -ItemType Directory -Path $temporaryCheckout | Out-Null
  git -C $temporaryCheckout init --quiet
  git -C $temporaryCheckout remote add origin $WorkerRepository
  git -C $temporaryCheckout fetch --quiet --depth 1 origin $WorkerCommit
  git -C $temporaryCheckout checkout --quiet --detach FETCH_HEAD
  $temporaryCheckout
} else {
  (Resolve-Path -LiteralPath $SourceDirectory).Path
}
$resolvedCommit = (git -C $checkout rev-parse HEAD).Trim()
if ($resolvedCommit -ne $WorkerCommit) {
  throw "Workflow worker commit mismatch: expected $WorkerCommit, got $resolvedCommit"
}
$dirtySource = [string](git -C $checkout status --porcelain)
if (-not $AllowDirtySource -and -not [string]::IsNullOrWhiteSpace($dirtySource)) {
  throw "Workflow worker source is dirty; commit the fork change or pass -AllowDirtySource for local-only validation."
}

$cargo = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
if (-not (Test-Path -LiteralPath $cargo -PathType Leaf)) {
  $cargo = "cargo"
}
& $cargo build --locked --release -p wisp-cli --manifest-path (Join-Path $checkout "Cargo.toml")
if ($LASTEXITCODE -ne 0) { throw "Workflow worker build failed." }

$sourceBinary = Join-Path $checkout "target\release\wisp-science.exe"
if (-not (Test-Path -LiteralPath $sourceBinary -PathType Leaf)) {
  throw "Workflow worker binary was not produced."
}
$destinationBinary = Join-Path $resourceRoot "rice-workflow-worker.exe"
Copy-Item -LiteralPath $sourceBinary -Destination $destinationBinary -Force
Copy-Item -LiteralPath (Join-Path $checkout "LICENSE") -Destination (Join-Path $noticeRoot "LICENSE-AGPL-3.0.txt") -Force

$assetMappings = @(
  @{ Source = "skills"; Destination = "skills" },
  @{ Source = "python"; Destination = "python" },
  @{ Source = "r"; Destination = "r" },
  @{ Source = "seed"; Destination = "seed" },
  @{ Source = "mcp-servers\bio-tools"; Destination = "mcp-servers\bio-tools" }
)
foreach ($mapping in $assetMappings) {
  $source = Join-Path $checkout $mapping.Source
  if (-not (Test-Path -LiteralPath $source -PathType Container)) {
    throw "Required workflow resource is missing: $($mapping.Source)"
  }
  $destination = Join-Path $engineResourceRoot $mapping.Destination
  if (Test-Path -LiteralPath $destination) {
    Remove-Item -LiteralPath $destination -Recurse -Force
  }
  New-Item -ItemType Directory -Force -Path $destination | Out-Null
  Get-ChildItem -LiteralPath $source -Force | Copy-Item -Destination $destination -Recurse -Force
}

$hash = (Get-FileHash -LiteralPath $destinationBinary -Algorithm SHA256).Hash.ToLowerInvariant()
$resourceLines = foreach ($file in Get-ChildItem -LiteralPath $engineResourceRoot -Recurse -File) {
  $relative = [System.IO.Path]::GetRelativePath($engineResourceRoot, $file.FullName).Replace("\", "/")
  $fileHash = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
  "$relative`t$fileHash`n"
}
[string[]]$sortedResourceLines = $resourceLines
[System.Array]::Sort($sortedResourceLines, [System.StringComparer]::Ordinal)
$resourceBytes = [System.Text.Encoding]::UTF8.GetBytes(($sortedResourceLines -join ""))
$resourceDigestBytes = [System.Security.Cryptography.SHA256]::HashData($resourceBytes)
$resourceHash = [System.Convert]::ToHexString($resourceDigestBytes).ToLowerInvariant()
$version = [string](& $destinationBinary --version 2>$null | Select-Object -First 1)
# The worker CLI rejects `--version` (exit code 1); clear it so the post-script
# `exit $LASTEXITCODE` appended by GitHub Actions does not fail a completed step.
$global:LASTEXITCODE = 0
if ([string]::IsNullOrWhiteSpace($version)) {
  $version = "wisp-science 1.8.0"
}
$manifest = [ordered]@{
  schema = "rice.workflow.worker-build.v1"
  worker = "rice-workflow-worker"
  engine = "wisp"
  engine_version = [string]$version
  fork_repository = $WorkerRepository
  fork_commit = $resolvedCommit
  source_dirty = -not [string]::IsNullOrWhiteSpace($dirtySource)
  upstream_repository = "https://github.com/xuzhougeng/wisp-science"
  protocol = "wisp.agent-rpc.v1"
  sha256 = $hash
  resources_sha256 = $resourceHash
  license = "AGPL-3.0-only"
  built_at = [DateTime]::UtcNow.ToString("o")
}
$manifest | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $noticeRoot "worker-build.json") -Encoding utf8NoBOM
Write-Host "Prepared workflow worker $resolvedCommit"
Write-Host "sha256: $hash"
Write-Host "resources sha256: $resourceHash"
} finally {
  if ($temporaryCheckout -and (Test-Path -LiteralPath $temporaryCheckout)) {
    Remove-Item -LiteralPath $temporaryCheckout -Recurse -Force -ErrorAction SilentlyContinue
  }
}
