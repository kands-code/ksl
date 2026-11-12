#!/usr/bin/env bash
# 从 ksl-catalog 导出内置函数名单, 生成 highlights.scm 的对应段.
# 只改写 BEGIN/END 之间. 名单与 tests/names.rs 的一致性由该测试校验.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
crate="$(dirname "$here")"
repo="$(dirname "$(dirname "$crate")")"
query="$crate/queries/highlights.scm"

if [ ! -f "$query" ]; then
    echo "缺少文件: $query" >&2
    exit 1
fi

dump() {
    cargo run --quiet --manifest-path "$repo/Cargo.toml" -p ksl-catalog --bin dump -- "$1"
}

if ! grep -qx '; BEGIN builtins' "$query" || ! grep -qx '; END builtins' "$query"; then
    echo "查询文件缺少 BEGIN/END 标记: $query" >&2
    exit 1
fi

names="$(dump builtins | LC_ALL=C sort -u)"

if [ -z "$names" ]; then
    echo "未从 catalog 导出任何内置函数名" >&2
    exit 1
fi

block="$(mktemp)"
trap 'rm -f "$block" "$query.tmp"' EXIT

{
    echo '; BEGIN builtins'
    echo '((identifier) @function.builtin'
    echo ' (#any-of? @function.builtin'
    printf '%s\n' "$names" | sed 's/.*/  "&"/'
    echo '))'
    echo '; END builtins'
} > "$block"

awk -v block="$block" '
    /^; BEGIN builtins$/ {
        while ((getline line < block) > 0) print line
        close(block)
        skipping = 1
        next
    }
    /^; END builtins$/ {
        skipping = 0
        next
    }
    !skipping { print }
' "$query" > "$query.tmp"

mv "$query.tmp" "$query"

count="$(printf '%s\n' "$names" | wc -l | tr -d ' ')"
echo "已更新 $query, 共 $count 个内置函数名."
