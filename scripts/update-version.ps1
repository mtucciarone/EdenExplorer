param (
  [string]$version
)

Write-Host "🔄 Updating Cargo.toml to version $version"

# Replace version in Cargo.toml
(Get-Content Cargo.toml) -replace 'version = ".*"', "version = `"$version`"" |
  Set-Content Cargo.toml

# Verify
Write-Host "✅ Updated Cargo.toml:"
Select-String 'version =' Cargo.toml

# Commit the change
git config user.name "github-actions"
git config user.email "github-actions@github.com"

git add Cargo.toml
git commit -m "chore(release): $version [skip ci]"