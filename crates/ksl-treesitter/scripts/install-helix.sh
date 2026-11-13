#!/usr/bin/env bash
# 编译 ksl 语法并安装到 Helix 运行时目录.
# 只写 runtime 文件; 语言登记由 .helix/languages.toml 提供.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
crate="$(dirname "$here")"
helix_config="${XDG_CONFIG_HOME:-$HOME/.config}/helix"
runtime="$helix_config/runtime"

if ! command -v hx >/dev/null 2>&1; then
    echo "未找到 hx, 请先安装 Helix." >&2
    exit 1
fi

# 借 hx 自身构建语法, 临时目录用完即删.
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/helix/runtime/grammars"
{
    echo 'use-grammars = { only = ["ksl"] }'
    cat "$crate/.helix/languages.toml"
    printf '\n[[grammar]]\nname = "ksl"\nsource = { path = "%s" }\n' "$crate"
} > "$tmp/helix/languages.toml"

echo "编译 ksl 语法..."
XDG_CONFIG_HOME="$tmp" XDG_CACHE_HOME="$tmp/cache" hx --grammar build

mkdir -p "$runtime/grammars" "$runtime/queries/ksl"

# 先写临时文件再改名, 避免覆盖 Helix 正在映射的 ksl.so.
install_file() {
    local src="$1"
    local dst="$2"
    cp "$src" "$dst.tmp"
    mv -f "$dst.tmp" "$dst"
}

install_file "$tmp/helix/runtime/grammars/ksl.so" "$runtime/grammars/ksl.so"
for query in highlights locals indents textobjects; do
    install_file "$crate/queries/$query.scm" "$runtime/queries/ksl/$query.scm"
done

echo "已安装到 $runtime"
echo
echo "从 crate 目录启动 hx 以加载工作区配置:"
echo "  cd \"$crate\" && hx test/drift/tour.ksl"
