param([switch]$NoAsio, [switch]$Package)
$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Push-Location -LiteralPath $projectRoot
try {
    . (Join-Path $PSScriptRoot 'environment.ps1')
    if (-not $NoAsio) {
        if (-not $env:LIBCLANG_PATH) {
            $clangDirectory = 'C:\Program Files\LLVM\bin'
            if (Test-Path -LiteralPath (Join-Path $clangDirectory 'libclang.dll')) {
                $env:LIBCLANG_PATH = $clangDirectory
            }
        }
        if (-not $env:CPAL_ASIO_DIR) {
            # asio-sys's downloader does not quote paths containing spaces.
            # Fetch the official SDK here with PowerShell's literal path arguments.
            $sdkDownload = Join-Path $projectRoot 'target\asio-sdk-download'
            $sdkDirectory = Join-Path $sdkDownload 'ASIOSDK'
            if (-not (Test-Path -LiteralPath (Join-Path $sdkDirectory 'common\asio.h'))) {
                New-Item -ItemType Directory -Force -Path $sdkDownload | Out-Null
                $archivePath = Join-Path $sdkDownload 'sdk.zip'
                Invoke-WebRequest -Uri 'https://www.steinberg.net/asiosdk' -OutFile $archivePath
                Expand-Archive -LiteralPath $archivePath -DestinationPath $sdkDownload -Force
            }
            if (-not (Test-Path -LiteralPath (Join-Path $sdkDirectory 'common\asio.h'))) {
                throw 'ASIO SDK layout changed. Set CPAL_ASIO_DIR to the SDK directory containing common/asio.h.'
            }
            $env:CPAL_ASIO_DIR = $sdkDirectory
        }
    }
    $cargoArguments = @('build', '--release', '--locked')
    if ($NoAsio) { $cargoArguments += '--no-default-features' }
    & cargo @cargoArguments
    if ($LASTEXITCODE -ne 0) { throw 'Rust build failed.' }
    if ($Package) {
        $packageName = if ($NoAsio) { 'Shredinini-0.1.0-windows-x64-wasapi' } else { 'Shredinini-0.1.0-windows-x64' }
        $packageDirectory = Join-Path $projectRoot "dist\$packageName"
        New-Item -ItemType Directory -Force -Path $packageDirectory | Out-Null
        Copy-Item -LiteralPath (Join-Path $projectRoot 'target\release\shredinini.exe') -Destination $packageDirectory -Force
        foreach ($document in @('README.md', 'VALIDATION.md', 'LICENSE', 'THIRD_PARTY_NOTICES.md')) {
            Copy-Item -LiteralPath (Join-Path $projectRoot $document) -Destination $packageDirectory -Force
        }
        if (-not $NoAsio) {
            Copy-Item -LiteralPath (Join-Path $env:CPAL_ASIO_DIR 'LICENSE.txt') -Destination (Join-Path $packageDirectory 'ASIO-SDK-LICENSE.txt') -Force
        }
        $zipPath = Join-Path $projectRoot "dist\$packageName.zip"
        Compress-Archive -Path (Join-Path $packageDirectory '*') -DestinationPath $zipPath -Force
        Write-Output "Portable build: $packageDirectory"
        Write-Output "Archive: $zipPath"
    }
} finally { Pop-Location }
