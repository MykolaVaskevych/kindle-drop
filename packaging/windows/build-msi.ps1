# Build the release .exe and wrap it in an .msi, writing both into dist/.
#
# Needs: the Rust MSVC toolchain, Strawberry Perl (for the vendored OpenSSL
# build), and the WiX v5 CLI:
#     dotnet tool install --global wix --version 5.0.2
#
#     powershell -ExecutionPolicy Bypass -File packaging\windows\build-msi.ps1
$ErrorActionPreference = 'Stop'

$repo = Resolve-Path "$PSScriptRoot\..\.."
Set-Location $repo
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { 'target' }
$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches.Groups[1].Value
$name = "kindle-drop-$version-x86_64-windows"

Write-Host "==> Building kindle-drop $version (vendored OpenSSL)..."
cargo build --release --locked --features windows-release
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

Write-Host "==> Building $name.msi..."
New-Item -ItemType Directory -Force dist | Out-Null
Push-Location packaging\windows
try {
    wix build kindle-drop.wxs -arch x64 `
        -d "Version=$version" `
        -d "BinDir=$repo\$target\release" `
        -d "RepoDir=$repo" `
        -o "$repo\dist\$name.msi"
    if ($LASTEXITCODE -ne 0) { throw "wix build failed" }
} finally {
    Pop-Location
}

# Bare executable too, for people who'd rather not install anything.
Copy-Item "$target\release\kindle-drop.exe" "dist\$name.exe"

Get-ChildItem dist -Filter "$name.*" | Where-Object Extension -ne '.sha256' | ForEach-Object {
    $hash = (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower()
    "$hash  $($_.Name)" | Set-Content -NoNewline -Encoding ascii "$($_.FullName).sha256"
}
Get-ChildItem dist
