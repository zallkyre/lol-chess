$ErrorActionPreference = "Stop"

$repo = "zallkyre/lol-chess"
$zipUrl = "https://github.com/$repo/archive/refs/heads/main.zip"
$sfFallback = "https://github.com/official-stockfish/Stockfish/releases/latest/download/stockfish-windows-x86-64-universal.zip"
$dir = Join-Path $env:LOCALAPPDATA "LolChess"
$tmp = Join-Path $env:TEMP ("lolchess-" + [guid]::NewGuid().ToString("N"))

function Say($text, $color = "Gray") {
    Write-Host $text -ForegroundColor $color
}

Say ""
Say "  LOL CHESS" "Green"
Say "  installing to $dir" "DarkGray"
Say ""

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Say "  rust not found - installing toolchain" "Yellow"
    $rustup = Join-Path $tmp "rustup-init.exe"
    New-Item -ItemType Directory -Force -Path $tmp | Out-Null
    Invoke-WebRequest -UseBasicParsing -Uri "https://win.rustup.rs/x86_64" -OutFile $rustup
    & $rustup -y --default-toolchain stable --profile minimal
    $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        throw "cargo still missing after rustup"
    }
} else {
    Say "  rust ok" "DarkGray"
}

New-Item -ItemType Directory -Force -Path $tmp | Out-Null

if (-not (Test-Path (Join-Path $dir "lichess\pieces\cburnett\wK.svg"))) {
    Say "  downloading source" "DarkGray"
    $zip = Join-Path $tmp "src.zip"
    Invoke-WebRequest -UseBasicParsing -Uri $zipUrl -OutFile $zip
    Expand-Archive -LiteralPath $zip -DestinationPath $tmp -Force
    $inner = Get-ChildItem -Path $tmp -Directory | Where-Object { $_.Name -like "lol-chess-*" } | Select-Object -First 1
    if (-not $inner) { throw "source folder not found in archive" }
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    Copy-Item -Path (Join-Path $inner.FullName "*") -Destination $dir -Recurse -Force
} else {
    Say "  source ok" "DarkGray"
}

$engine = Join-Path $dir "engine"
New-Item -ItemType Directory -Force -Path $engine | Out-Null
if (-not (Test-Path (Join-Path $engine "stockfish.exe"))) {
    Say "  downloading stockfish" "DarkGray"
    $sfUrl = $sfFallback
    try {
        $rel = Invoke-RestMethod -Headers @{ "User-Agent" = "lol-chess" } `
            -Uri "https://api.github.com/repos/official-stockfish/Stockfish/releases/latest"
        $asset = $rel.assets |
            Where-Object { $_.name -like "stockfish-windows-x86-64*.zip" } |
            Select-Object -First 1
        if ($asset) { $sfUrl = $asset.browser_download_url }
    }
    catch { }
    $sfZip = Join-Path $tmp "sf.zip"
    Invoke-WebRequest -UseBasicParsing -Uri $sfUrl -OutFile $sfZip
    $sfDir = Join-Path $tmp "sf"
    Expand-Archive -LiteralPath $sfZip -DestinationPath $sfDir -Force
    $exes = @(Get-ChildItem -Path $sfDir -Recurse -Filter "stockfish*.exe")
    $exe = $exes | Where-Object { $_.Name -like "*avx2*" } | Select-Object -First 1
    if (-not $exe) { $exe = $exes | Where-Object { $_.Name -like "*bmi2*" } | Select-Object -First 1 }
    if (-not $exe) { $exe = $exes | Select-Object -First 1 }
    if (-not $exe) { throw "stockfish.exe not found in archive" }
    Copy-Item -LiteralPath $exe.FullName -Destination (Join-Path $engine "stockfish.exe") -Force
} else {
    Say "  stockfish ok" "DarkGray"
}

Say "  building (a few minutes)" "DarkGray"
Push-Location $dir
try {
    & cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    Copy-Item -Path (Join-Path $dir "target\release\lol-chess.exe") -Destination (Join-Path $dir "lol-chess.exe") -Force
}
finally {
    Pop-Location
}

$exePath = Join-Path $dir "lol-chess.exe"
if (-not (Test-Path $exePath)) { throw "build produced no exe" }

$shell = New-Object -ComObject WScript.Shell
$lnk = Join-Path ([Environment]::GetFolderPath("Desktop")) "LOL Chess.lnk"
$s = $shell.CreateShortcut($lnk)
$s.TargetPath = $exePath
$s.WorkingDirectory = $dir
$s.IconLocation = (Join-Path $dir "assets\icon.ico")
$s.Description = "LOL Chess"
$s.Save()

Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue

Say ""
Say "  done - shortcut: $lnk" "Green"
Say "  launching" "DarkGray"
Start-Process -FilePath $exePath -WorkingDirectory $dir
