// Page-side helper for headless captures (tools/web/cdp_shot.mjs waits for the title).
//
// ezviz tags a figure's canvas with data-ezviz-backend="webgpu" | "webgl2" once it can draw, or
// data-ezviz-error="..." if the GPU could not be initialized. signalDone() waits for that, lets
// `frames` more animation frames pass, optionally waits for an element with id `waitFor` (an
// <img> that must have loaded), then sets document.title = "done:<backend>" (or "done:error").
export async function signalDone({ frames = 60, waitFor = null } = {}) {
  const raf = () => new Promise((r) => requestAnimationFrame(r));
  let canvas;
  while (!(canvas = document.querySelector('canvas[data-ezviz-backend], canvas[data-ezviz-error]'))) await raf();
  if (canvas.dataset.ezvizError) {
    console.error('ezviz canvas error:', canvas.dataset.ezvizError);
    document.title = 'done:error';
    return;
  }
  for (let i = 0; i < frames; i++) await raf();
  if (waitFor) {
    for (;;) {
      const el = document.getElementById(waitFor);
      if (el && (el.complete ?? true)) break;
      await raf();
    }
  }
  document.title = 'done:' + canvas.dataset.ezvizBackend;
}
