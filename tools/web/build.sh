#!/bin/sh
# Builds an example for the browser into examples/web/pkg/ (serve examples/web/ and open <example>.html).
#
#   tools/web/build.sh <example> [more examples...]
#
# Installs the matching wasm-bindgen CLI (0.2.129, pinned in Cargo.toml) into .tools/ on first use.
# The wasm name section (function names in panic backtraces, ~1.4 MB) is dropped unless
# SCIPLOT_WASM_NAMES=1. Release builds for wasm32 carry no DWARF (it would be stripped anyway).
# Features: `window` only (no CPU rasterizer, resvg is ~1 MB of wasm); override with
# SCIPLOT_WASM_FEATURES="window cpu-png".
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
  CARGO_PROFILE_RELEASE_DEBUG=0 cargo build --release --target wasm32-unknown-unknown --example "$ex" \
    --no-default-features --features "${SCIPLOT_WASM_FEATURES:-window}"
  names=--remove-name-section
  [ "${SCIPLOT_WASM_NAMES:-0}" = 1 ] && names=
  "$WB" --target web --no-typescript $names --out-dir examples/web/pkg \
    "target/wasm32-unknown-unknown/release/examples/$ex.wasm"
  wasm="examples/web/pkg/${ex}_bg.wasm"
  echo "$ex: $(wc -c < "$wasm" | tr -d ' ') bytes wasm, $(gzip -9c "$wasm" | wc -c | tr -d ' ') bytes gzipped"
done
