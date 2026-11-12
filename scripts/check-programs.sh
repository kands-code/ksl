#!/usr/bin/env bash
# 检查示例与基准的 ksl 程序能否通过类型检查.
#
# 用法:
#   scripts/check-programs.sh

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

cargo build -p ksl-cli --quiet
binary="target/debug/ksl"

status=0
count=0
while IFS= read -r file; do
    count=$((count + 1))
    if ! "$binary" check "$file" >/dev/null; then
        echo "程序检查失败: $file" >&2
        status=1
    fi
done < <(find examples bench -type f -name '*.ksl' | sort)

if [ "$count" -eq 0 ]; then
    echo "未找到可检查的 ksl 程序." >&2
    exit 1
fi

exit "$status"
