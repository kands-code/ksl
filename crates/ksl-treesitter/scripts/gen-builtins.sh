#!/usr/bin/env bash
# 从 ksl-core 内置注册表生成 highlights.scm 的内置函数名单.
# 只改写 BEGIN/END 之间; 一致性由 tests/names.rs 校验.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
crate="$(dirname "$here")"
repo="$(dirname "$(dirname "$crate")")"
source="$repo/crates/ksl-core/src/builtin/registry.rs"
query="$crate/queries/highlights.scm"

for file in "$source" "$query"; do
    if [ ! -f "$file" ]; then
        echo "缺少文件: $file" >&2
        exit 1
    fi
done

if ! grep -qx '; BEGIN builtins' "$query" || ! grep -qx '; END builtins' "$query"; then
    echo "查询文件缺少 BEGIN/END 标记: $query" >&2
    exit 1
fi

# 注册表条目在名字后紧跟逗号; 压平换行后按 `( "名字" ,` 提取,
# 用括号和逗号限定以排除 intern("IOError") 这类内部字符串.
names="$(
    tr '\n' ' ' < "$source" \
        | grep -oE '\([[:space:]]*"[A-Za-z0-9_]+"[[:space:]]*,' \
        | sed -E 's/.*"([A-Za-z0-9_]+)".*/\1/' \
        | LC_ALL=C sort -u
)"

if [ -z "$names" ]; then
    echo "未从签名表提取到任何名字: $source" >&2
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
