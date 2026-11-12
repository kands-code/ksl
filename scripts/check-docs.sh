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

# 注释中不允许全角标点与破折号. 只检查注释行, 字符串与代码块不在范围内.
# 覆盖 Rust, ksl 程序与配置文件; 配置文件的注释以 `#` 开头.
check_comment_punctuation() {
    local pattern='[，。；：！？、（）“”‘’《》—–…]'
    local hits
    hits="$(
        grep -rnE "^[[:space:]]*//.*$pattern" --include='*.rs' crates || true
        grep -rnE "^[[:space:]]*#.*$pattern" --include='*.ksl' crates examples bench plugins || true
        grep -rnE "^[[:space:]]*#.*$pattern" \
            --include='*.toml' --include='*.yml' --include='*.yaml' \
            --include='*.sh' --include='*.json' \
            --exclude-dir=target --exclude-dir=.git . || true
    )"
    if [[ -n "$hits" ]]; then
        echo "注释包含全角标点或破折号:" >&2
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
