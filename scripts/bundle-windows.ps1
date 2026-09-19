param(
    [string]$Version = ""
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

if ([string]::IsNullOrWhiteSpace($Version)) {
    $VersionLine = Select-String -Path "crates/laika-app/Cargo.toml" -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
    if (-not $VersionLine) { throw "Could not read the Laika version" }
    $Version = $VersionLine.Matches[0].Groups[1].Value
}

$Name = "Laika-$Version-windows-x64"
$Stage = Join-Path $Root "dist/$Name"
$Zip = Join-Path $Root "dist/$Name.zip"
$Checksum = "$Zip.sha256"

if (-not (Test-Path "target/release/laika.exe")) {
    throw "target/release/laika.exe is missing; run cargo build --release -p laika-app -p laika-render first"
}

if (Test-Path $Stage) { Remove-Item -Recurse -Force $Stage }
if (Test-Path $Zip) { Remove-Item -Force $Zip }
New-Item -ItemType Directory -Force $Stage | Out-Null
New-Item -ItemType Directory -Force (Join-Path $Stage "Resources/fonts") | Out-Null

Copy-Item "target/release/laika.exe" $Stage
Copy-Item "target/release/laika-render.exe" $Stage
Copy-Item "README.md" $Stage
Copy-Item "assets/icon/Laika.ico" $Stage
Copy-Item "assets/fonts/*" (Join-Path $Stage "Resources/fonts")
if (Test-Path "fixtures/raw") {
    New-Item -ItemType Directory -Force (Join-Path $Stage "Resources/samples") | Out-Null
    Copy-Item "fixtures/raw/*" (Join-Path $Stage "Resources/samples")
}

@"
Laika $Version for Windows x64

Run laika.exe. Windows SmartScreen may show an unrecognized-publisher warning
because this development build is not code-signed; choose More info -> Run anyway.

laika-render.exe is the command-line reference renderer. Keep Resources beside
the executables so gallery fonts and optional sample files can be found.
"@ | Set-Content -Encoding UTF8 (Join-Path $Stage "WINDOWS-README.txt")

Compress-Archive -Path $Stage -DestinationPath $Zip -CompressionLevel Optimal
$Hash = (Get-FileHash -Algorithm SHA256 $Zip).Hash.ToLowerInvariant()
"$Hash  $Name.zip" | Set-Content -Encoding ASCII $Checksum
Write-Host "Created $Zip"
Write-Host "SHA256 $Hash"
