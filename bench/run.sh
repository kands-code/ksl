#!/usr/bin/env bash
# 用 hyperfine 逐个跑基准用例.
#
# 测量的是整个进程, 含启动与退出; 用例本身的耗时在几十到百毫秒量级, 启动占比可忽略.
#
# 用法:
#   bench/run.sh            逐个用例测量并打印
#   bench/run.sh --json     额外把结果导出到 bench/results/

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(dirname "$here")"
binary="$root/target/release/ksl"

if ! command -v hyperfine >/dev/null 2>&1; then
    echo "未找到 hyperfine, 请先安装." >&2
    exit 1
fi

if [ ! -x "$binary" ]; then
    echo "未找到 release 产物, 先执行 cargo build --release -p ksl-cli" >&2
    exit 1
fi

export_json=""
if [ "${1:-}" = "--json" ]; then
    export_json=1
    shift
fi

warmup=3
runs=10

for case in "$here"/*.ksl; do
    name="$(basename "$case" .ksl)"
    echo "== $name =="
    # hyperfine 2.0 的命令是一个整串, 内部按命令行拆分.
    # -N 即 --shell=none, 不经 shell 启动, 减少外部干扰;
    # --prepare sync 在每次计时前同步磁盘, 使缓存状态一致.
    command="\"$binary\" run \"$case\""
    if [ -n "$export_json" ]; then
        mkdir -p "$here/results"
        hyperfine -N --prepare sync --warmup "$warmup" --runs "$runs" \
            --export-json "$here/results/$name.json" \
            "$command"
    else
        hyperfine -N --prepare sync --warmup "$warmup" --runs "$runs" \
            "$command"
    fi
done
