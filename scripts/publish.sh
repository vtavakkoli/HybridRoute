#!/usr/bin/env bash
set -euo pipefail

repository="${1:-vtavakkoli/HybridRoute}"
description="High-performance policy-constrained hybrid semantic API router written in Rust"

command -v git >/dev/null || { echo "Git is required" >&2; exit 1; }
command -v gh >/dev/null || { echo "GitHub CLI is required; install it and run: gh auth login" >&2; exit 1; }
gh auth status >/dev/null

if [[ ! -d .git ]]; then
  git init -b main
fi

git add --all
if ! git diff --cached --quiet; then
  git commit -m "Initial HybridRoute semantic API router"
fi

if gh repo view "$repository" >/dev/null 2>&1; then
  git remote get-url origin >/dev/null 2>&1 || git remote add origin "https://github.com/${repository}.git"
  git push -u origin main
else
  gh repo create "$repository" --public --description "$description" --source . --remote origin --push
fi

echo "Published: https://github.com/${repository}"
