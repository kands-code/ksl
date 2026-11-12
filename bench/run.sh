#!/usr/bin/env bash
# 用 hyperfine 逐个运行基准用例.
#
# 基准测量整个进程, 含启动与退出; 用例耗时远大于启动开销.
# 用例顺序即报告顺序. 每个用例文件须含 `# @result` 行, 脚本以它校验运行结果.
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

# 报告顺序. 未登记的 .ksl 文件会使脚本失败.
cases=(
    call
    list_build
    list_scan
    dict
    match
    string
    actor
    timer
    timeout
)

for file in "$here"/*.ksl; do
    name="$(basename "$file" .ksl)"
    if [[ ! " ${cases[*]} " =~ " ${name} " ]]; then
        echo "基准用例未登记: $name" >&2
        exit 1
    fi
done

export_json=""
if [ "${1:-}" = "--json" ]; then
    export_json=1
    shift
fi

warmup=3
runs=10

for name in "${cases[@]}"; do
    case_file="$here/$name.ksl"
    if [ ! -f "$case_file" ]; then
        echo "缺少基准用例: $name" >&2
        exit 1
    fi
    expected="$(sed -n 's/^# @result //p' "$case_file")"
    if [ -z "$expected" ]; then
        echo "基准用例缺少 # @result: $name" >&2
        exit 1
    fi
    actual="$("$binary" run "$case_file")"
    if [ "$actual" != "$expected" ]; then
        echo "基准结果不符: $name" >&2
        echo "  期望: $expected" >&2
        echo "  实际: $actual" >&2
        exit 1
    fi

    echo "== $name =="
    # hyperfine 的命令参数是整串, 由其内部按命令行拆分, 路径需自行加引号.
    # -N (--shell=none) 不经 shell 启动, 减少外部干扰.
    # --prepare sync 在每次计时前同步磁盘, 使缓存状态一致.
    command="\"$binary\" run \"$case_file\""
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
