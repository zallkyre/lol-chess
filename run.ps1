# LOL Chess - launcher
# Prefers the built binary, falls back to `cargo run --release`.
$ErrorActionPreference = "Stop"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path

$candidates = @(
    (Join-Path $here "lol-chess.exe"),
    (Join-Path $here "target\release\lol-chess.exe")
)
foreach ($exe in $candidates) {
    if (Test-Path -LiteralPath $exe) {
        Start-Process -FilePath $exe -WorkingDirectory $here
        exit
    }
}

Set-Location -LiteralPath $here
cargo run --release
