#!/usr/bin/env bash
# 从 ksl-catalog 生成 .helix/ksl.mtsx 中的特殊形式, 操作, 内置函数与内置类型名单.
# 只改写四组 BEGIN/END 之间; 修改语言后须运行.
# 高亮分组: 核心, Actor 与命名注册归特殊形式; 容器构造器 Dict/Set 与其余容器操作归操作.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
crate="$(dirname "$here")"
repo="$(dirname "$(dirname "$crate")")"
target="$crate/.helix/ksl.mtsx"

if [ ! -f "$target" ]; then
    echo "缺少文件: $target" >&2
    exit 1
fi

dump() {
    cargo run --quiet --manifest-path "$repo/Cargo.toml" -p ksl-catalog --bin dump -- "$1"
}

builtins="$(dump builtins | LC_ALL=C sort -u)"
operations="$(dump operations | LC_ALL=C sort -u)"
registry_ops="$(dump registry-ops | LC_ALL=C sort -u)"
special_forms="$(dump special-forms | LC_ALL=C sort -u)"
types="$(dump types | LC_ALL=C sort -u)"

# 分组调整: Dict/Set 归操作, 命名注册归特殊形式.
special_forms="$(
    printf '%s\n' "$special_forms" "$registry_ops" \
        | grep -vE '^(Dict|Set)$' \
        | sed '/^$/d' \
        | LC_ALL=C sort -u
)"
operations="$(
    printf '%s\n' "$operations" "Dict" "Set" \
        | grep -vFxf <(printf '%s\n' "$registry_ops") \
        | sed '/^$/d' \
        | LC_ALL=C sort -u
)"

for list in special_forms operations registry_ops builtins types; do
    if [ -z "${!list}" ]; then
        echo "未导出到 $list 名单" >&2
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
