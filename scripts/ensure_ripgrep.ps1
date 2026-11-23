$ErrorActionPreference = 'Stop'

$rg = Get-Command rg -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
if ($rg) {
    & $rg.Path --version
    exit $LASTEXITCODE
}

$version = '15.2.0'
$asset = "ripgrep-$version-x86_64-pc-windows-msvc"
$archive = Join-Path $env:RUNNER_TEMP "$asset.zip"
$sha256 = '71b2fef860abe467217a538ff31de02f5258807c0129f771846f87bd029aafc5'
$url = "https://github.com/BurntSushi/ripgrep/releases/download/$version/$asset.zip"
curl.exe --fail --location --retry 2 --silent --show-error --output $archive $url
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne $sha256) {
    throw "Checksum mismatch for $asset.zip"
}

Expand-Archive -LiteralPath $archive -DestinationPath $env:RUNNER_TEMP -Force
$directory = Join-Path $env:RUNNER_TEMP $asset
& (Join-Path $directory 'rg.exe') --version
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Add-Content -LiteralPath $env:GITHUB_PATH -Value $directory -Encoding utf8
