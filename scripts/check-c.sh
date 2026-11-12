#!/usr/bin/env bash
# 检查或修复手写 C 源文件的格式.
#
# 用法:
#   scripts/check-c.sh        只检查, 供提交钩子使用
#   scripts/check-c.sh --fix  就地格式化

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

if ! command -v clang-format >/dev/null 2>&1; then
    echo "缺少 clang-format, 请先安装" >&2
    exit 1
fi

# 只检查手写源文件; parser.c 与 src/tree_sitter 由 tree-sitter 生成.
files=(
    crates/ksl-treesitter/src/scanner.c
)

if [[ "${1:-}" == "--fix" ]]; then
    clang-format --style=file -i "${files[@]}"
else
    clang-format --style=file --dry-run --Werror "${files[@]}"
fi
