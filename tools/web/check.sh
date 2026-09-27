#!/bin/sh
# End-to-end browser check (macOS + Chrome, see docs/research/web-backend.md §4):
#  1. builds the web examples (tools/web/build.sh) and the native reference PNG of web_static;
#  2. serves examples/web/ and captures every page in headless Chrome at DPR 2, on WebGPU and
#     with WebGL2 forced (WebGPU disabled in Chrome, and the app-level ?backend=gl);
#  3. compares the static figure on the canvas and from Figure::to_png_bytes_async with the
#     native PNG export (tools/web/pngdiff.mjs);
#  4. drives wheel zoom, drag pan, touch pinch/pan, ctrl+wheel (trackpad pinch), rectangle zoom,
#     double-click reset and hover through CDP input (tools/web/input_actions.json);
#  5. lists console errors.
# Images and logs go to out/web/. Usage: tools/web/check.sh [--no-build]
set -eu
cd "$(dirname "$0")/../.."
PORT=${PORT:-8791}
OUT=out/web
mkdir -p "$OUT"

if [ "${1:-}" != "--no-build" ]; then
  tools/web/build.sh web_static web_lorenz web_grayscott
fi
cargo run --quiet --release --example web_static
cp out/web_static_native.png "$OUT/static_native.png"

python3 -m http.server "$PORT" --bind 127.0.0.1 --directory examples/web >/dev/null 2>&1 &
SERVER=$!
trap 'kill $SERVER 2>/dev/null' EXIT INT TERM
sleep 1
URL="http://127.0.0.1:$PORT"

webgpu() { node tools/web/cdp_shot.mjs "$@" -- --force-device-scale-factor=2; }
webgl() { NO_UNSAFE=1 node tools/web/cdp_shot.mjs "$@" -- --disable-features=WebGPUService --force-device-scale-factor=2; }

: > "$OUT/console.log"
for page in web_lorenz web_grayscott; do
  echo "== $page"
  webgpu "$URL/$page.html" "$OUT/${page}_webgpu.png" --dpr=0 --width=1000 --height=460 --extra-ms=1500 | tee -a "$OUT/console.log"
  webgl "$URL/$page.html" "$OUT/${page}_webgl2.png" --dpr=0 --width=1000 --height=460 --extra-ms=1500 | tee -a "$OUT/console.log"
done

echo "== web_static"
webgpu "$URL/web_static.html" "$OUT/static_webgpu.png" --dpr=0 --width=720 --height=720 --extra-ms=300 | tee -a "$OUT/console.log"
webgl "$URL/web_static.html" "$OUT/static_webgl2.png" --dpr=0 --width=720 --height=720 --extra-ms=300 | tee -a "$OUT/console.log"
webgpu "$URL/web_static.html?backend=gl" "$OUT/static_glquery.png" --dpr=0 --width=720 --height=720 --extra-ms=300 | tee -a "$OUT/console.log"

echo "== pixel diffs against the native PNG (1440x720 device px)"
for m in webgpu webgl2 glquery; do
  printf '%-8s canvas  ' "$m"
  node tools/web/pngdiff.mjs "$OUT/static_$m.png" "$OUT/static_native.png" --crop-a=0,0,1440,720 --out="$OUT/diff_canvas_$m.png"
  printf '%-8s async   ' "$m"
  node tools/web/pngdiff.mjs "$OUT/static_$m.png" "$OUT/static_native.png" --crop-a=0,720,1440,720 --out="$OUT/diff_png_$m.png"
done

echo "== input (WebGPU, then WebGL2)"
webgpu "$URL/web_static.html" "$OUT/input_final_webgpu.png" --dpr=0 --width=720 --height=720 --extra-ms=300 \
  --actions=tools/web/input_actions.json | tee -a "$OUT/console.log" | grep -v '^\[cdp\] action'
mkdir -p "$OUT/gl"
sed 's|out/web/|out/web/gl/|' tools/web/input_actions.json > "$OUT/gl/actions.json"
webgl "$URL/web_static.html" "$OUT/gl/input_final.png" --dpr=0 --width=720 --height=720 --extra-ms=300 \
  --actions="$OUT/gl/actions.json" | tee -a "$OUT/console.log" | grep -v '^\[cdp\] action'

echo "== console errors"
grep -E '^\[(exception|console\.error|log\.error)\]|timeout' "$OUT/console.log" || echo "(none)"
