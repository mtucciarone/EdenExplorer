$ErrorActionPreference = "Stop"

Write-Host "🔨 Building EdenExplorer..."

cargo build --release --verbose

mkdir -Force release

if (Test-Path "target/release/EdenExplorer.exe") {
  Copy-Item "target/release/EdenExplorer.exe" "release/EdenExplorer.exe"
} elseif (Test-Path "target/release/eden_explorer.exe") {
  Copy-Item "target/release/eden_explorer.exe" "release/EdenExplorer.exe"
} else {
  Write-Error "❌ No binary found"
}

if (Test-Path "README.md") { Copy-Item "README.md" "release/" }
if (Test-Path "LICENSE") { Copy-Item "LICENSE" "release/" }

Write-Host "✅ Build + packaging complete"