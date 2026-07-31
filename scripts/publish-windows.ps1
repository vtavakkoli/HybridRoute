param(
    [string]$Repository = "vtavakkoli/HybridRoute",
    [string]$Description = "High-performance policy-constrained hybrid semantic API router written in Rust"
)

$ErrorActionPreference = "Stop"

if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    throw "Git is required. Install Git and run this script again."
}
if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
    throw "GitHub CLI is required. Install it and run 'gh auth login'."
}

gh auth status | Out-Null

if (-not (Test-Path .git)) {
    git init -b main
}

git add --all
if (-not (git diff --cached --quiet)) {
    git commit -m "Initial HybridRoute semantic API router"
}

$existing = gh repo view $Repository --json nameWithOwner 2>$null
if (-not $existing) {
    gh repo create $Repository --public --description $Description --source . --remote origin --push
} else {
    if (-not (git remote get-url origin 2>$null)) {
        git remote add origin "https://github.com/$Repository.git"
    }
    git push -u origin main
}

Write-Host "Published: https://github.com/$Repository"
