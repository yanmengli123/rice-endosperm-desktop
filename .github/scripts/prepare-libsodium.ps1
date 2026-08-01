$ErrorActionPreference = "Stop"

$version = "1.0.22"
$expectedSha256 = "3e03a726fac4bc09cb61d8f29d658ef7a5eca0811de59082130414f7ca2e4279"
$dist = Join-Path $env:RUNNER_TEMP "libsodium-dist"
$archive = Join-Path $dist "libsodium-$version-msvc.zip"
$extracted = Join-Path $dist "extracted"

New-Item -ItemType Directory -Path $dist -Force | Out-Null
Invoke-WebRequest "https://github.com/jedisct1/libsodium/releases/download/$version-RELEASE/libsodium-$version-msvc.zip" -OutFile $archive

$actualSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $archive).Hash.ToLowerInvariant()
if ($actualSha256 -ne $expectedSha256) {
  throw "libsodium SHA-256 mismatch: $actualSha256"
}

Expand-Archive -LiteralPath $archive -DestinationPath $extracted -Force
$libraryDirectory = Join-Path $extracted "libsodium\x64\Release\v143\static"
if (-not (Test-Path -LiteralPath (Join-Path $libraryDirectory "libsodium.lib"))) {
  throw "libsodium static library was not found after extraction"
}
"SODIUM_LIB_DIR=$libraryDirectory" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
