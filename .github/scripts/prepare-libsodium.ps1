$ErrorActionPreference = "Stop"

$version = "1.0.22"
$expectedSha256 = "3e03a726fac4bc09cb61d8f29d658ef7a5eca0811de59082130414f7ca2e4279"
$tempRoot = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { $env:TEMP }
$dist = Join-Path $tempRoot "daoxin-libsodium-dist"
$archive = Join-Path $dist "libsodium-$version-msvc.zip"
$extracted = Join-Path $dist "extracted"

New-Item -ItemType Directory -Path $dist -Force | Out-Null
Invoke-WebRequest "https://github.com/jedisct1/libsodium/releases/download/$version-RELEASE/libsodium-$version-msvc.zip" -OutFile $archive

$actualSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $archive).Hash.ToLowerInvariant()
if ($actualSha256 -ne $expectedSha256) {
  throw "libsodium SHA-256 mismatch: $actualSha256"
}

Expand-Archive -LiteralPath $archive -DestinationPath $extracted -Force
$libraryDirectory = Join-Path $extracted "libsodium\x64\Release\v143\dynamic"
$libraryPath = Join-Path $libraryDirectory "libsodium.lib"
$dllPath = Join-Path $libraryDirectory "libsodium.dll"
if (-not (Test-Path -LiteralPath $libraryPath) -or -not (Test-Path -LiteralPath $dllPath)) {
  throw "libsodium dynamic library was not found after extraction"
}

$repositoryRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$resourceDirectory = Join-Path $repositoryRoot "src-tauri\resources"
New-Item -ItemType Directory -Path $resourceDirectory -Force | Out-Null
Copy-Item -LiteralPath $dllPath -Destination (Join-Path $resourceDirectory "libsodium.dll") -Force

$env:SODIUM_LIB_DIR = $libraryDirectory
$env:SODIUM_SHARED = "1"
$env:PATH = "$libraryDirectory;$env:PATH"

if ($env:GITHUB_ENV) {
  "SODIUM_LIB_DIR=$libraryDirectory" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
  "SODIUM_SHARED=1" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
}
if ($env:GITHUB_PATH) {
  $libraryDirectory | Out-File -FilePath $env:GITHUB_PATH -Encoding utf8 -Append
}

Write-Host "Prepared verified libsodium dynamic runtime: $libraryDirectory"
