# Creates (or refreshes) the "LOL Chess" desktop shortcut.
$ErrorActionPreference = "Stop"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$run = Join-Path $here "run.ps1"
$name = "LOL Chess"

$shell = New-Object -ComObject WScript.Shell
$desktop = [Environment]::GetFolderPath("Desktop")
$lnk = Join-Path $desktop "$name.lnk"
$s = $shell.CreateShortcut($lnk)
$s.TargetPath = (Get-Command powershell.exe).Source
$s.Arguments = "-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$run`""
$s.WorkingDirectory = $here
$s.IconLocation = (Join-Path $here "assets\icon.ico")
$s.Description = "LOL Chess"
$s.Save()
Write-Host "Shortcut written: $lnk"
