#!/usr/bin/env bash
# 启用仓库内的 Git 钩子.

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

chmod +x .githooks/pre-commit scripts/check-docs.sh scripts/install-hooks.sh
git config core.hooksPath .githooks
echo "已启用 .githooks"
