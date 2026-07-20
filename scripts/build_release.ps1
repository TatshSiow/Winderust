param(
    [string]$TargetDir = "target"
)

$ErrorActionPreference = "Stop"

if (-not $env:GPUI_FXC_PATH -or -not (Test-Path -LiteralPath $env:GPUI_FXC_PATH)) {
    $fxcCommand = Get-Command fxc.exe -ErrorAction SilentlyContinue
    if ($fxcCommand) {
        $env:GPUI_FXC_PATH = $fxcCommand.Source
    } else {
        $programFilesX86 = [Environment]::GetFolderPath(
            [Environment+SpecialFolder]::ProgramFilesX86
        )
        $sdkBin = Join-Path $programFilesX86 "Windows Kits\10\bin"
        $fxcCandidate = Get-ChildItem -LiteralPath $sdkBin -Directory -ErrorAction SilentlyContinue |
            Sort-Object Name -Descending |
            ForEach-Object { Join-Path $_.FullName "x64\fxc.exe" } |
            Where-Object { Test-Path -LiteralPath $_ } |
            Select-Object -First 1

        if (-not $fxcCandidate) {
            throw "fxc.exe was not found. Install a Windows SDK or set GPUI_FXC_PATH."
        }

        $env:GPUI_FXC_PATH = $fxcCandidate
    }
}

Write-Host "Using fxc.exe at $env:GPUI_FXC_PATH"
& cargo build --release --locked --target-dir $TargetDir
exit $LASTEXITCODE
