#!/usr/bin/env bash
# 长 REPL 会话基准.
#
# 生成 N 条顶层绑定输入, 一次性喂给 REPL, 测量整段输入的耗时.
# 覆盖会话克隆, 类型表合并与全局环境增长; 默认 N=3000.
#
# 用法:
#   bench/repl.sh [N]
#   bench/repl.sh --json [N]   # 额外导出到 bench/results/repl.json

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(dirname "$here")"
binary="$root/target/release/ksl"

export_json=""
if [ "${1:-}" = "--json" ]; then
    export_json=1
    shift
fi
count="${1:-3000}"

if ! command -v hyperfine >/dev/null 2>&1; then
    echo "未找到 hyperfine, 请先安装." >&2
    exit 1
fi

if [ ! -x "$binary" ]; then
    echo "未找到 release 产物, 先执行 cargo build --release -p ksl-cli" >&2
    exit 1
fi

input="$(mktemp "${TMPDIR:-/tmp}/ksl-repl.XXXXXX")"
trap 'rm -f "$input"' EXIT

for i in $(seq 0 $((count - 1))); do
    printf '@Let[x%s, %s];\n' "$i" "$i"
done > "$input"
printf 'x%s\n' "$((count - 1))" >> "$input"

command="\"$binary\" repl < \"$input\" > /dev/null 2>/dev/null"
if [ -n "$export_json" ]; then
    mkdir -p "$here/results"
    hyperfine --shell=default --prepare sync --warmup 2 --runs 5 \
        --export-json "$here/results/repl.json" "$command"
else
    hyperfine --shell=default --prepare sync --warmup 2 --runs 5 "$command"
fi
