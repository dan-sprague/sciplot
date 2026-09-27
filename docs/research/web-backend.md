# Web backend research: winit 0.30.13 + wgpu 30.0.1 on wasm (verified in headless Chrome 153, M3)

Verified by running a probe app, not just from docs: one wasm binary with wgpu features `webgpu` +
`webgl`, a winit app mounted in a canvas, 4x MSAA, instanced quads with a vertex buffer bound at two
offsets, `first_instance = 128`, dynamic uniform offsets, an `R32Float` texture read with `textureLoad` in
the vertex shader, `textureSampleLevel` in the vertex shader, and a 50 % black quad blended over white.
It rendered pixel-identically on WebGPU and WebGL2 (blend reads exactly (128,128,128) on both, so gamma-space
blending with non-sRGB targets works). rAF runs at 60 fps headless.
Probe sources: `docs/research/web-probe/` (lib.rs, Cargo.toml, index.html). Capture tool: `tools/web/cdp_shot.mjs`.

## 1. winit 0.30.13 on the web
- Window attrs (`WindowAttributesExtWebSys`): `with_canvas(Option<HtmlCanvasElement>)` (None: winit makes a
  canvas but does not insert it), `with_append(true)` (append to `<body>`), `with_prevent_default(bool)`
  (default on), `with_focusable(bool)` (default on, tabindex=0). Create windows in `resumed()`. `Window` is
  Send + Sync on web.
- Use `EventLoopExtWebSys::spawn_app(app)` (takes the app by value, `'static`, returns immediately).
  `run_app` throws a JS exception for control flow (makes `await init()` reject). One EventLoop per page:
  several figures need one app with several windows; later figures reach it via `EventLoopProxy`.
- Pacing: `request_redraw()` = requestAnimationFrame (coalesced). Continuous animation: keep
  `ControlFlow::Wait` and call `request_redraw()` at the end of `RedrawRequested`. Avoid `Poll` (postTask
  spin, not vsynced). winit requests a frame after every `Resized`. rAF pauses in hidden tabs: clamp `dt`.
- HiDPI: `scale_factor()` = devicePixelRatio (zoom changes it: `ScaleFactorChanged` then `Resized`).
  `Resized` reports device pixels (ResizeObserver devicePixelContentBox; Safari: contentRect x dpr).
  winit never writes canvas.width/height; wgpu's `configure` does. `inner_size()` is 0x0 until the first
  ResizeObserver callback, which may arrive before or after async GPU init: handle both, skip 0 sizes.
- Events (CDP input at DPR 2): mouse `CursorMoved` in physical px; `MouseWheel { PixelDelta }` with sign
  flipped and scaled by DPR (DOM deltaY=+120 -> y=-240), phase always `Moved`; Chrome reports mouse and
  trackpad as pixel deltas (Firefox mouse wheels give `LineDelta`). Trackpad pinch = ctrl+wheel
  (`ModifiersChanged(CONTROL)` + wheel); no gesture events on web. `ModifiersChanged` only while the canvas
  has focus. Keyboard on the canvas only (needs focus; winit focuses it at creation and on pointerdown with
  prevent_default). Touch: `WindowEvent::Touch` per pointer id; no pinch event (compute from two touches),
  touch gives no `CursorMoved`. `Occluded` from IntersectionObserver.
- `EventLoopProxy::send_event` never re-enters synchronously (microtask / queued); safe from `spawn_local`.
- Gotchas: prevent_default blocks page scroll on wheel over the canvas and Tab/space while focused
  (`window.set_prevent_default()` toggles). Set CSS `touch-action: none`. No border/padding/transform on
  the canvas. **Always give the canvas a CSS size** (or `with_inner_size`, applied as CSS only if the
  canvas is already in the DOM): without it every configure doubles the canvas at DPR 2 until textures
  become invalid. Clamp to `max_texture_dimension_2d`.

## 2. wgpu 30.0.1 on wasm
- Features: `default-features = false, features = ["std", "parking_lot", "wgsl", "webgpu", "webgl"]` under
  `[target.'cfg(target_arch = "wasm32")'.dependencies]` (`wgsl` is required for WebGL: naga WGSL -> GLSL).
- Backend choice: plain `Instance::new` with `BROWSER_WEBGPU` picks WebGPU whenever `navigator.gpu`
  exists even if `requestAdapter()` returns null (no fallback). Use
  `wgpu::util::new_instance_with_webgpu_detection(desc).await`. Force GL with `desc.backends =
  Backends::GL`. `adapter.get_info().backend` = `BrowserWebGpu` | `Gl`. Decide the backend BEFORE
  `create_surface` (a canvas with a webgpu context can't give a webgl2 context).
- Async init: window -> surface -> `request_adapter { compatible_surface: Some(&surface) }` (mandatory for
  WebGL2) -> `request_device`, all in `wasm_bindgen_futures::spawn_local`, result delivered via
  `EventLoopProxy` (pattern of the v30.0.1 examples). `pollster` can't work on wasm.
- Surface: `create_surface(Arc<Window>)` -> `Surface<'static>`; without winit `SurfaceTarget::Canvas`.
  Present with `queue.present(frame)`. WebGPU formats `[Bgra8Unorm, Rgba8Unorm, Rgba16Float]` (preferred
  canvas format first: bgra8unorm in Chrome/Mac). **WebGL formats `[Rgba8UnormSrgb, Rgba8Unorm,
  Rgba16Float]`: sRGB first, no Bgra** — never take `formats[0]`, pick `Rgba8Unorm | Bgra8Unorm`
  explicitly. WebGL `Bgra8Unorm` textures map to GL BGRA which WebGL2 lacks: use `Rgba8Unorm` offscreen on
  GL. Target format must be per-surface. Both backends: alpha `[Opaque]`, present `[Fifo]` (WebGPU panics on
  Mailbox/Immediate; AutoVsync ok). WebGL2 context: antialias false, alpha true, premultipliedAlpha true —
  clear to opaque. `SurfaceColorSpace::Srgb` works on both. WebGL surface usage is COLOR_TARGET only (window
  dumps must render offscreen).
- MSAA 4x on WebGL2 works (Rgba8Unorm has MULTISAMPLE_X4 + RESOLVE; MAX_SAMPLES 4 on ANGLE/Metal).
- `Limits::downlevel_webgl2_defaults()`: 2D textures 2048; storage buffers/textures 0; uniform binding
  16 KiB; max vertex buffer array stride 255; 8 vertex buffers; 8 dynamic uniform buffers; offset alignment
  256; 15 inter-stage variables; 4 color attachments; no compute. Request
  `downlevel_webgl2_defaults().using_resolution(adapter.limits())`. Real adapters: WebGL ubo_align 32,
  ubo_size 16384, tex2d 16384; WebGPU ubo_align 256, ubo_size 65536.
- WebGL2 checks: instancing works; `first_instance` emulated (works); R32Float + textureLoad works
  (declare `Float { filterable: false }`); textureSampleLevel in VS works; dynamic uniform offsets work.
  Missing downlevel flags: VERTEX_STORAGE, INDEPENDENT_BLEND, BASE_VERTEX, FULL_DRAW_INDEX_UINT32,
  SURFACE_VIEW_FORMATS; not set: UNRESTRICTED_INDEX_BUFFER (index buffers can't have other usages),
  BUFFER_BINDINGS_NOT_16_BYTE_ALIGNED (uniform binding sizes multiple of 16). **One sampler per texture.**
- **On WebGL an adapter/device belongs to one canvas's GL context** (`surface_capabilities` is None for
  other canvases): the shared `Gpu` works for multiple canvases only on WebGPU; WebGL needs one device per
  canvas.
- **wgpu types are not Send/Sync on wasm** unless feature `fragile-send-sync-non-atomic-wasm`:
  `static OnceLock<Arc<Gpu>>` won't compile on wasm without it (or use a thread_local on wasm).
- Readback (`to_png_bytes`) must be async on web (`map_async` + await; `poll` is a no-op on web).
- Size: WebGPU+WebGL 4.68 MB (1.30 MB gz); WebGPU only 0.92 MB (236 KB gz), opt-level 2, no wasm-opt.

## 3. Toolchain
- `rustup target add wasm32-unknown-unknown` (was not installed at research time).
- Resolved: wasm-bindgen **0.2.129**, wasm-bindgen-futures 0.4.79, js-sys/web-sys 0.3.106, web-time 1.1.0,
  glow 0.17.0, naga 30.0.1, console_error_panic_hook 0.1.7, console_log 1.1.0. wgpu 30.0.1 needs
  wasm-bindgen >= 0.2.127. Pin `wasm-bindgen = "=0.2.129"`; the CLI must match exactly:
  `cargo install wasm-bindgen-cli --version 0.2.129 --locked` (~3 min).
- Pipeline (cargo examples work; `fn main` runs on `init()`):
  ```sh
  cargo build --release --target wasm32-unknown-unknown --example web_lorenz
  wasm-bindgen --target web --no-typescript --out-dir examples/web/pkg \
    target/wasm32-unknown-unknown/release/examples/web_lorenz.wasm
  ```
  ```html
  <style>#sciplot{width:640px;height:400px;display:block;touch-action:none;outline:none}</style>
  <canvas id="sciplot"></canvas>
  <script type="module">import init from './pkg/web_lorenz.js'; await init();</script>
  ```
- Extra crates: `console_error_panic_hook::set_once()`, `console_log::init_with_level(Info)`,
  `web_time::Instant` everywhere (`std::time::Instant::now()` panics on wasm32-unknown-unknown). sciplot's
  normal deps don't pull getrandom on wasm; demos using `rand` 0.9 need
  `getrandom = { version = "0.3", features = ["wasm_js"] }`. parking_lot compiles on wasm but a contended or
  re-entrant lock panics there.

## 4. Headless Chrome on macOS (Chrome 153)
- Serve: `python3 -m http.server 8765 --bind 127.0.0.1 --directory <dir>`.
- Capture (recommended): `node tools/web/cdp_shot.mjs "http://127.0.0.1:8765/<page>" out.png --dpr=0
  --width=800 --height=500 --extra-ms=1000 -- --force-device-scale-factor=2`. The page should set
  `document.title = "done..."` once it has rendered. WebGPU works headless on the Mac without flags.
  HiDPI: `--force-device-scale-factor=2` (CDP deviceScaleFactor alone leaves ResizeObserver at 1x).
  The plain CLI `--screenshot` works but Chrome never exits: wrap with
  `perl -e 'alarm 20; exec @ARGV' ...`. Don't use `--virtual-time-budget` (captures before GPU init).
- Force WebGL2: `--disable-features=WebGPUService` without `--enable-unsafe-webgpu` (run cdp_shot with
  `NO_UNSAFE=1`); or app-level `?backend=gl`. `--disable-gpu` kills WebGL2 too; `--use-angle=swiftshader`
  is software (slow).
- Console logs: collected by cdp_shot (Runtime.consoleAPICalled, exceptionThrown, Log.entryAdded); CLI:
  `--enable-logging=stderr --v=0`, grep `INFO:CONSOLE`.
- Linux CI (untested): `--headless=new --use-angle=vulkan --enable-features=Vulkan --disable-vulkan-surface
  --enable-unsafe-webgpu`.

## 5. Recent changes / bugs
- wgpu 30.0.1 fixes a 30.0.0 panic when `requestAdapter()` fails on WebGPU.
- Browser WebGPU (gpuweb wiki, 2026-08-13): Chrome 113+ Mac/Win/ChromeOS, Linux only Intel Gen12+ (144) and
  NVIDIA Wayland (147); Firefox 141 Windows, 147 macOS, not Linux/Android yet; Safari 26+ all Apple OSes.
  Hence the WebGL2 fallback matters for Linux and Android Firefox.

Sources: docs.rs winit 0.30.13 platform::web, wgpu 30.0.1 util::new_instance_with_webgpu_detection,
wgpu v30.0.1 examples/features/src/framework.rs and xtask/src/run_wasm.rs, wgpu CHANGELOG, Chrome headless
docs, gpuweb Implementation-Status wiki, local crate sources under ~/.cargo/registry.
