param([string]$TargetDir = "target")

$ErrorActionPreference = "Stop"

& cargo build --release --locked --target-dir $TargetDir
exit $LASTEXITCODE
