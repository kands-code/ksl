#!/usr/bin/env bash
# 从 ksl-core 生成 .helix/ksl.mtsx 中的特殊形式, 操作, 内置函数与内置类型名单.
# 只改写四组 BEGIN/END 之间; 修改语言后须运行.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
crate="$(dirname "$here")"
repo="$(dirname "$(dirname "$crate")")"
core="$repo/crates/ksl-core/src"
registry="$core/builtin/registry.rs"
special="$core/parser/special.rs"
symbol="$core/symbol.rs"
ops="$core/ast/ops.rs"
reserved="$core/check/reserved.rs"
target="$crate/.helix/ksl.mtsx"

for file in "$registry" "$special" "$symbol" "$ops" "$reserved" "$target"; do
    if [ ! -f "$file" ]; then
        echo "缺少文件: $file" >&2
        exit 1
    fi
done

# 与 gen-builtins.sh 相同的提取规则.
builtins="$(
    tr '\n' ' ' < "$registry" \
        | grep -oE '\([[:space:]]*"[A-Za-z0-9_]+"[[:space:]]*,' \
        | sed -E 's/.*"([A-Za-z0-9_]+)".*/\1/' \
        | LC_ALL=C sort -u
)"

operations="$(
    grep -oE 'OpSpec::new\("[A-Za-z0-9_]+"' "$ops" \
        | sed -E 's/.*"([A-Za-z0-9_]+)".*/\1/' \
        | LC_ALL=C sort -u
)"

# 特殊形式由 special_form 分派与 symbol.rs 驻留表共同确定.
special_fields="$(
    sed -n '/fn special_form(/,/fn form_let(/p' "$special" \
        | grep -oE 'kw\.[a-z_]+_' \
        | sed 's/^kw\.//' \
        | LC_ALL=C sort -u
)"

special_forms="$(
    while IFS= read -r field; do
        [ -n "$field" ] || continue
        sed -n "s/^[[:space:]]*${field}:[[:space:]]*intern(\"\([^\"]*\)\").*/\1/p" "$symbol"
    done <<< "$special_fields"
)"

# Dict 与 Set 由 special_form 的字面量分支处理.
special_forms+=$'\n'"$(
    sed -n '/fn special_form(/,/fn form_let(/p' "$special" \
        | grep -oE 'resolve\(node\)\.as_ref\(\) == "[A-Za-z0-9_]+"' \
        | sed -E 's/.*"([A-Za-z0-9_]+)".*/\1/'
)"

special_forms="$(printf '%s\n' "$special_forms" | sed '/^$/d' | LC_ALL=C sort -u)"

types="$(
    sed -n '/pub const BUILTIN_TYPE_NAMES/,/^];/p' "$reserved" \
        | grep -oE '"[A-Za-z0-9_]+"' \
        | tr -d '"' \
        | LC_ALL=C sort -u
)"

for list in special_forms operations builtins types; do
    if [ -z "${!list}" ]; then
        echo "未提取到 $list 名单" >&2
        exit 1
    fi
done

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# 每行 8 个名字, 便于 diff.
write_block() {
    printf '%s\n' "$2" | awk '
        { names[NR] = $0 }
        END {
            per_line = 8
            for (i = 1; i <= NR; i += per_line) {
                printf "            \""
                for (j = i; j < i + per_line && j <= NR; j++) {
                    if (j > i) printf " "
                    printf "%s", names[j]
                }
                printf "\"\n"
            }
        }
    ' > "$1"
}

write_block "$tmp/special-forms" "$special_forms"
write_block "$tmp/operations" "$operations"
write_block "$tmp/builtins" "$builtins"
write_block "$tmp/types" "$types"

replace_block() {
    local begin="$1"
    local end="$2"
    local block="$3"

    if ! grep -qF "$begin" "$target" || ! grep -qF "$end" "$target"; then
        echo "目标文件缺少标记: $begin / $end" >&2
        exit 1
    fi

    awk -v begin="$begin" -v end="$end" -v block="$block" '
        index($0, begin) {
            print
            while ((getline line < block) > 0) print line
            close(block)
            skipping = 1
            next
        }
        index($0, end) {
            print
            skipping = 0
            next
        }
        !skipping { print }
    ' "$target" > "$target.tmp"
    mv "$target.tmp" "$target"
}

replace_block "// BEGIN special forms" "// END special forms" "$tmp/special-forms"
replace_block "// BEGIN operations" "// END operations" "$tmp/operations"
replace_block "// BEGIN builtins" "// END builtins" "$tmp/builtins"
replace_block "// BEGIN types" "// END types" "$tmp/types"

count() {
    printf '%s\n' "$1" | wc -l | tr -d ' '
}

echo "已更新 $target"
echo "  特殊形式: $(count "$special_forms")"
echo "  操作名:   $(count "$operations")"
echo "  内置函数: $(count "$builtins")"
echo "  内置类型: $(count "$types")"
