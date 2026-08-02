[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"

foreach ($name in "GITHUB_REF_NAME", "GITHUB_REPOSITORY", "RUNNER_TEMP") {
    $value = [Environment]::GetEnvironmentVariable($name)
    if ([string]::IsNullOrWhiteSpace($value)) {
        throw "$name is required."
    }
}

$package = Get-Content -LiteralPath "package.json" -Raw -Encoding UTF8 | ConvertFrom-Json
$version = [string]$package.version
$expectedTag = "v$version"
if ($env:GITHUB_REF_NAME -ne $expectedTag) {
    throw "Release tag $($env:GITHUB_REF_NAME) does not match package version $version."
}

$nsisInstaller = @(Get-ChildItem "src-tauri/target/release/bundle/nsis" -File -Filter "*.exe")
$msiInstaller = @(Get-ChildItem "src-tauri/target/release/bundle/msi" -File -Filter "*.msi")
if ($nsisInstaller.Count -ne 1 -or $msiInstaller.Count -ne 1) {
    throw "Expected exactly one NSIS installer and one MSI installer."
}

$nsisSignaturePath = "$($nsisInstaller[0].FullName).sig"
if (-not (Test-Path -LiteralPath $nsisSignaturePath -PathType Leaf)) {
    throw "NSIS updater signature was not produced."
}

$nsisAssetName = "Daoxin-Zhixi_${version}_x64-setup.exe"
$msiAssetName = "Daoxin-Zhixi_${version}_x64.msi"
$releaseAssetNames = @(gh release view $expectedTag --json assets --jq ".assets[].name")
foreach ($assetName in $nsisAssetName, $msiAssetName) {
    if ($releaseAssetNames -notcontains $assetName) {
        throw "Expected release asset was not uploaded: $assetName"
    }
}

$signature = (Get-Content -LiteralPath $nsisSignaturePath -Raw -Encoding UTF8).Trim()
if ([string]::IsNullOrWhiteSpace($signature)) {
    throw "NSIS updater signature is empty."
}

$downloadUrl = "https://github.com/$($env:GITHUB_REPOSITORY)/releases/download/$expectedTag/$nsisAssetName"
$manifest = [ordered]@{
    version = $version
    notes = "稻芯智析 v$version"
    pub_date = [DateTimeOffset]::UtcNow.ToString("yyyy-MM-dd'T'HH:mm:ss'Z'")
    platforms = [ordered]@{
        "windows-x86_64" = [ordered]@{
            signature = $signature
            url = $downloadUrl
        }
    }
}

$manifestPath = Join-Path $env:RUNNER_TEMP "latest.json"
$manifestJson = $manifest | ConvertTo-Json -Depth 8
[System.IO.File]::WriteAllText($manifestPath, $manifestJson, [System.Text.UTF8Encoding]::new($false))

$checksumPath = Join-Path $env:RUNNER_TEMP "SHA256SUMS.txt"
$checksumLines = @(
    "$((Get-FileHash -Algorithm SHA256 -LiteralPath $nsisInstaller[0].FullName).Hash.ToLowerInvariant())  $nsisAssetName"
    "$((Get-FileHash -Algorithm SHA256 -LiteralPath $msiInstaller[0].FullName).Hash.ToLowerInvariant())  $msiAssetName"
)
[System.IO.File]::WriteAllLines($checksumPath, $checksumLines, [System.Text.Encoding]::ASCII)

gh release upload $expectedTag $manifestPath $checksumPath --clobber
if ($LASTEXITCODE -ne 0) {
    throw "Failed to upload release metadata."
}

$finalAssetNames = @(gh release view $expectedTag --json assets --jq ".assets[].name")
foreach ($assetName in "latest.json", "SHA256SUMS.txt") {
    if ($finalAssetNames -notcontains $assetName) {
        throw "Release metadata verification failed: $assetName is missing."
    }
}
