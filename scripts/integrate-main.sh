#!/usr/bin/env bash
set -euo pipefail
git fetch origin main
git merge --no-commit --no-ff -X ours origin/main
