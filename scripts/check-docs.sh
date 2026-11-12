#!/usr/bin/env bash
# 检查或修复 Markdown 与 TOML 的格式.
#
# 用法:
#   scripts/check-docs.sh        只检查, 供 CI 与提交钩子使用
#   scripts/check-docs.sh --fix  就地格式化

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

for tool in prettier taplo; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "缺少 $tool, 请先安装" >&2
        exit 1
    fi
done

# 注释中只允许半角标点; 字符串与代码块不在此检查范围内.
check_comment_punctuation() {
    local pattern='[，。；：！？、（）“”‘’《》—…]'
    local hits
    hits="$(
        grep -rnE "^[[:space:]]*//.*$pattern" --include='*.rs' crates || true
        grep -rnE "^[[:space:]]*#.*$pattern" --include='*.ksl' crates examples bench || true
    )"
    if [[ -n "$hits" ]]; then
        echo "注释包含全角标点:" >&2
        echo "$hits" >&2
        return 1
    fi
}

if [[ "${1:-}" == "--fix" ]]; then
    prettier --write '**/*.md'
    taplo fmt
else
    prettier --check '**/*.md'
    taplo fmt --check
    taplo lint
fi

check_comment_punctuation
