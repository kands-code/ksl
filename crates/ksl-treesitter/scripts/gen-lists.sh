#!/usr/bin/env bash
# 从 ksl-catalog 生成全部编辑器与语法名单.
#
# 生成位置:
#   grammar.js 的操作族数组
#   queries/highlights.scm 的特殊形式, 容器操作, 内置函数与类型名单
#   .helix/ksl.mtsx 的特殊形式, 容器操作, 内置函数与类型名单
#
# 只改写 BEGIN/END 之间的内容. 特殊形式的语法规则仍手写,
# 形式名覆盖由 tests/names.rs 校验.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
crate="$(dirname "$here")"
repo="$(dirname "$(dirname "$crate")")"
grammar="$crate/grammar.js"
highlights="$crate/queries/highlights.scm"
mtsx="$crate/.helix/ksl.mtsx"

for file in "$grammar" "$highlights" "$mtsx"; do
    if [ ! -f "$file" ]; then
        echo "缺少文件: $file" >&2
        exit 1
    fi
done

dump() {
    cargo run --quiet --manifest-path "$repo/Cargo.toml" -p ksl-catalog --bin dump -- "$1"
}

builtins="$(dump builtins | LC_ALL=C sort -u)"
types="$(dump types | LC_ALL=C sort -u)"
primitives="$(dump primitive-types | LC_ALL=C sort -u)"
constructors="$(dump type-constructors | LC_ALL=C sort -u)"
named_types="$(dump named-types | LC_ALL=C sort -u)"
special_forms="$(
    dump special-forms \
        | grep -vE '^(Dict|Set)$' \
        | sed '/^$/d' \
        | LC_ALL=C sort -u
)"
op_families="$(dump op-families)"
operations="$(
    {
        printf '%s\n' "$op_families" | awk '{print $2}'
        printf 'Dict\nSet\n'
    } | sed '/^$/d' | LC_ALL=C sort -u
)"

for list in builtins types primitives constructors named_types special_forms operations op_families; do
    if [ -z "${!list}" ]; then
        echo "未导出到 $list 名单" >&2
        exit 1
    fi
done

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# 每行 8 个名字.
write_dense() {
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

write_highlights_keywords() {
    local capture="$1"
    local names="$2"
    local out="$3"
    {
        echo '['
        printf '%s\n' "$names" | sed 's/.*/  "&"/'
        echo "] $capture"
    } > "$out"
}

write_highlights_builtins() {
    local out="$1"
    {
        echo '((identifier) @function.builtin'
        echo ' (#any-of? @function.builtin'
        printf '%s\n' "$builtins" | sed 's/.*/  "&"/'
        echo '))'
    } > "$out"
}

write_highlights_type_names() {
    local capture="$1"
    local names="$2"
    local out="$3"
    {
        printf '((type_name) %s\n' "$capture"
        printf ' (#any-of? %s\n' "$capture"
        printf '%s\n' "$names" | sed 's/.*/  "&"/'
        echo '))'
        printf '((identifier) %s\n' "$capture"
        printf ' (#any-of? %s\n' "$capture"
        printf '%s\n' "$names" | sed 's/.*/  "&"/'
        echo '))'
    } > "$out"
}

write_grammar_ops() {
    local family out="$1"
    for family in LIST_OPS PAIR_OPS DICT_OPS SET_OPS OPTION_OPS; do
        case "$family" in
            LIST_OPS) echo '/// 列表操作族.' ;;
            PAIR_OPS) echo '/// 配对操作族.' ;;
            DICT_OPS) echo '/// Dict 操作族.' ;;
            SET_OPS) echo '/// Set 操作族.' ;;
            OPTION_OPS) echo '/// Option 与 Result 操作族.' ;;
        esac
        echo "const $family = ["
        printf '%s\n' "$op_families" \
            | awk -v family="$family" '$1 == family { print $2 }' \
            | sed "s/.*/  '&',/"
        echo '];'
        echo
    done > "$out"
}

write_dense "$tmp/special-forms" "$special_forms"
write_dense "$tmp/operations" "$operations"
write_dense "$tmp/builtins" "$builtins"
write_dense "$tmp/primitives" "$primitives"
write_dense "$tmp/constructors" "$constructors"
write_dense "$tmp/named-types" "$named_types"
write_highlights_keywords "@keyword" "$special_forms" "$tmp/highlight-special-forms"
write_highlights_keywords "@keyword.operator" "$operations" "$tmp/highlight-operations"
write_highlights_builtins "$tmp/highlight-builtins"
write_highlights_type_names "@type.builtin" "$primitives" "$tmp/highlight-primitives"
write_highlights_type_names "@type.constructor" "$constructors" "$tmp/highlight-constructors"
write_grammar_ops "$tmp/grammar-operations"

replace_block() {
    local begin="$1"
    local end="$2"
    local block="$3"
    local target="$4"

    if ! grep -qF "$begin" "$target" || ! grep -qF "$end" "$target"; then
        echo "目标文件缺少标记: $begin / $end ($target)" >&2
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
        index($0, end) { skipping = 0 }
        !skipping { print }
    ' "$target" > "$target.tmp"
    mv "$target.tmp" "$target"
}

replace_block \
    "// BEGIN operation families" \
    "// END operation families" \
    "$tmp/grammar-operations" \
    "$grammar"
replace_block \
    "; BEGIN special forms" \
    "; END special forms" \
    "$tmp/highlight-special-forms" \
    "$highlights"
replace_block \
    "; BEGIN container operations" \
    "; END container operations" \
    "$tmp/highlight-operations" \
    "$highlights"
replace_block \
    "; BEGIN builtins" \
    "; END builtins" \
    "$tmp/highlight-builtins" \
    "$highlights"
replace_block \
    "; BEGIN primitive types" \
    "; END primitive types" \
    "$tmp/highlight-primitives" \
    "$highlights"
replace_block \
    "; BEGIN type constructors" \
    "; END type constructors" \
    "$tmp/highlight-constructors" \
    "$highlights"
replace_block \
    "// BEGIN special forms" \
    "// END special forms" \
    "$tmp/special-forms" \
    "$mtsx"
replace_block \
    "// BEGIN operations" \
    "// END operations" \
    "$tmp/operations" \
    "$mtsx"
replace_block \
    "// BEGIN builtins" \
    "// END builtins" \
    "$tmp/builtins" \
    "$mtsx"
replace_block \
    "// BEGIN primitive types" \
    "// END primitive types" \
    "$tmp/primitives" \
    "$mtsx"
replace_block \
    "// BEGIN type constructors" \
    "// END type constructors" \
    "$tmp/constructors" \
    "$mtsx"
replace_block \
    "// BEGIN named types" \
    "// END named types" \
    "$tmp/named-types" \
    "$mtsx"

count() {
    printf '%s\n' "$1" | wc -l | tr -d ' '
}

echo "已更新编辑器与语法名单"
echo "  特殊形式: $(count "$special_forms")"
echo "  容器操作: $(count "$operations")"
echo "  内置函数: $(count "$builtins")"
echo "  原始类型: $(count "$primitives")"
echo "  类型构造器: $(count "$constructors")"
echo "  命名类型: $(count "$named_types")"
