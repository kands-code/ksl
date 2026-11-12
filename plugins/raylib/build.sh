#!/bin/sh
# 构建 raylib 插件并放入 plugin/.
#
# 用法:
#   ./build.sh
#   RAYLIB_DIR=/path/to/raylib ./build.sh
#
# 未设置 RAYLIB_DIR 时, build.rs 会从 vendor/ 下按平台选择 raylib 目录.

set -eu

cd "$(dirname "$0")"
repo="$(cd ../.. && pwd)"

cargo build --release --manifest-path Cargo.toml --target-dir "$repo/target"

case "$(uname -s)" in
    Darwin)
        built="$repo/target/release/libksl_raylib.dylib"
        target="plugin/libraylib.dylib"
        ;;
    Linux)
        built="$repo/target/release/libksl_raylib.so"
        target="plugin/libraylib.so"
        ;;
    *)
        built="$repo/target/release/ksl_raylib.dll"
        target="plugin/raylib.dll"
        ;;
esac

mkdir -p plugin
cp "$built" "$target"
