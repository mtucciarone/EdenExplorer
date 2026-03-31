$ErrorActionPreference = "Stop"

param (
  [string]$version
)

Write-Host "🔄 Updating Cargo.toml to version $version"

(Get-Content Cargo.toml) `
  -replace '^version = ".*"', "version = `"$version`"" |
  Set-Content Cargo.toml

Write-Host "✅ Updated Cargo.toml:"
Select-String '^version =' Cargo.toml

git config user.name "github-actions"
git config user.email "github-actions@github.com"

git add Cargo.toml
git commit -m "chore(release): $version [skip ci]"

git push origin HEAD:main