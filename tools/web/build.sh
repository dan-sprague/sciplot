#!/bin/sh
# Builds an example for the browser into examples/web/pkg/ (serve examples/web/ and open <example>.html).
#
#   tools/web/build.sh <example> [more examples...]
#
# Installs the matching wasm-bindgen CLI (0.2.129, pinned in Cargo.toml) into .tools/ on first use.
set -eu
cd "$(dirname "$0")/../.."
[ $# -ge 1 ] || { echo "usage: tools/web/build.sh <example>..." >&2; exit 2; }

WB=.tools/bin/wasm-bindgen
# In a git worktree, reuse the CLI installed in the main checkout's .tools/.
MAIN_WB="$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null || echo .git)/../.tools/bin/wasm-bindgen"
if ! "$WB" --version 2>/dev/null | grep -q '0\.2\.129'; then
  if "$MAIN_WB" --version 2>/dev/null | grep -q '0\.2\.129'; then
    WB=$MAIN_WB
  else
    cargo install wasm-bindgen-cli --version 0.2.129 --locked --root .tools
  fi
fi

for ex in "$@"; do
  cargo build --release --target wasm32-unknown-unknown --example "$ex" --features window
  "$WB" --target web --no-typescript --out-dir examples/web/pkg \
    "target/wasm32-unknown-unknown/release/examples/$ex.wasm"
  wasm="examples/web/pkg/${ex}_bg.wasm"
  echo "$ex: $(wc -c < "$wasm" | tr -d ' ') bytes wasm, $(gzip -9c "$wasm" | wc -c | tr -d ' ') bytes gzipped"
done
