param(
    [switch]$Release
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

foreach ($Tool in @("git", "rustup", "cargo")) {
    if (-not (Get-Command $Tool -ErrorAction SilentlyContinue)) {
        throw "$Tool is not on PATH. See docs/windows-port.md."
    }
}

$VsWhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
if (-not (Test-Path $VsWhere)) {
    throw "Visual Studio 2022 Build Tools were not found. Install Desktop development with C++."
}
$Vs = & $VsWhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if ([string]::IsNullOrWhiteSpace($Vs)) {
    throw "The MSVC x64 C++ tools are missing. Modify Visual Studio Build Tools and select Desktop development with C++."
}

rustup default stable
rustup update stable
cargo fetch --locked
cargo check --workspace --all-targets --locked
cargo test --workspace --locked

if ($Release) {
    cargo build --release -p laika-app -p laika-render --locked
    & "$PSScriptRoot\bundle-windows.ps1"
}

Write-Host "Windows bootstrap checks completed successfully."
