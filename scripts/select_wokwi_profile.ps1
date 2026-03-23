param(
    [ValidateSet("esp32-c3", "pi-pico", "arduino-uno")]
    [string]$Board = "esp32-c3"
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$profileRoot = Join-Path $repoRoot "wokwi-profiles"
$profileDir = Join-Path $profileRoot $Board

if (-not (Test-Path $profileDir)) {
    throw "Unknown Wokwi profile: $Board"
}

$tomlSource = Join-Path $profileDir "wokwi.toml"
$diagramSource = Join-Path $profileDir "diagram.json"
$tomlTarget = Join-Path $repoRoot "wokwi.toml"
$diagramTarget = Join-Path $repoRoot "diagram.json"

Copy-Item $tomlSource $tomlTarget -Force
Copy-Item $diagramSource $diagramTarget -Force

$firmwareDir = Join-Path $repoRoot ("target/wokwi/" + $Board)
if (-not (Test-Path $firmwareDir)) {
    New-Item -ItemType Directory -Path $firmwareDir -Force | Out-Null
}

Write-Output "Activated Wokwi profile: $Board"
Write-Output "Updated files: wokwi.toml, diagram.json"
Write-Output "Firmware folder: $firmwareDir"
