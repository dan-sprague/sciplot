// Headless-Chrome page capture over the DevTools protocol (no npm deps; Node >= 18).
// Usage: node cdp_shot.mjs <url> <out.png> [--wait-title=done] [--extra-ms=1000] [--dpr=2]
//        [--width=900] [--height=700] [--timeout-ms=30000] [-- <extra chrome flags...>]
// Waits until document.title starts with --wait-title (set by the page when it has rendered),
// then waits --extra-ms more (animation time), then captures the viewport to a PNG.
import { spawn } from 'node:child_process';
import { writeFileSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const argv = process.argv.slice(2);
const dd = argv.indexOf('--');
const extraFlags = dd >= 0 ? argv.slice(dd + 1) : [];
const args = dd >= 0 ? argv.slice(0, dd) : argv;
const opt = Object.fromEntries(args.filter(a => a.startsWith('--')).map(a => a.slice(2).split('=')));
const [url, out] = args.filter(a => !a.startsWith('--'));
const waitTitle = opt['wait-title'] ?? 'done';
const extraMs = +(opt['extra-ms'] ?? 1000);
const dpr = +(opt.dpr ?? 2);
const width = +(opt.width ?? 900), height = +(opt.height ?? 700);
const timeoutMs = +(opt['timeout-ms'] ?? 30000);
const chrome = process.env.CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';

const profile = mkdtempSync(join(process.env.TMPDIR ?? tmpdir(), 'cdp-prof-'));
const flags = [
  '--headless', `--user-data-dir=${profile}`, '--remote-debugging-pipe',
  '--no-first-run', '--no-default-browser-check', '--disable-background-networking',
  '--disable-component-update', '--disable-sync', '--mute-audio',
  `--window-size=${width},${height}`, ...(process.env.NO_UNSAFE ? [] : ['--enable-unsafe-webgpu']), ...extraFlags, 'about:blank',
];
const proc = spawn(chrome, flags, { stdio: ['ignore', 'ignore', 'pipe', 'pipe', 'pipe'] });
proc.stderr.on('data', d => { if (process.env.CHROME_STDERR) process.stderr.write(d); });
const toChrome = proc.stdio[3], fromChrome = proc.stdio[4];

let nextId = 1; const pending = new Map(); const listeners = [];
let buf = Buffer.alloc(0);
fromChrome.on('data', chunk => {
  buf = Buffer.concat([buf, chunk]);
  let i;
  while ((i = buf.indexOf(0)) >= 0) {
    const msg = JSON.parse(buf.subarray(0, i).toString('utf8')); buf = buf.subarray(i + 1);
    if (msg.id && pending.has(msg.id)) {
      const { resolve, reject } = pending.get(msg.id); pending.delete(msg.id);
      msg.error ? reject(new Error(JSON.stringify(msg.error))) : resolve(msg.result);
    } else for (const l of listeners) l(msg);
  }
});
const send = (method, params = {}, sessionId) => new Promise((resolve, reject) => {
  const id = nextId++; pending.set(id, { resolve, reject });
  toChrome.write(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }) + '\0');
});
const sleep = ms => new Promise(r => setTimeout(r, ms));

const logs = [];
listeners.push(m => {
  if (m.method === 'Runtime.consoleAPICalled')
    logs.push(`[console.${m.params.type}] ` + m.params.args.map(a => a.value ?? a.description ?? '').join(' '));
  else if (m.method === 'Runtime.exceptionThrown')
    logs.push(`[exception] ${m.params.exceptionDetails.exception?.description ?? m.params.exceptionDetails.text}`);
  else if (m.method === 'Log.entryAdded') logs.push(`[log.${m.params.entry.level}] ${m.params.entry.text}`);
});

let code = 0;
try {
  const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
  const { sessionId: s } = await send('Target.attachToTarget', { targetId, flatten: true });
  await send('Runtime.enable', {}, s); await send('Log.enable', {}, s); await send('Page.enable', {}, s);
  // deviceScaleFactor 0 = keep the real DSF (use --force-device-scale-factor for HiDPI: emulated DSF does not
  // reach ResizeObserver devicePixelContentBoxSize in Chrome 153, so winit would report CSS-px sizes).
  await send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: dpr, mobile: false }, s);
  await send('Page.navigate', { url }, s);
  const t0 = Date.now(); let title = '';
  while (Date.now() - t0 < timeoutMs) {
    const r = await send('Runtime.evaluate', { expression: 'document.title', returnByValue: true }, s);
    title = r.result.value ?? '';
    if (title.startsWith(waitTitle)) break;
    await sleep(100);
  }
  if (!title.startsWith(waitTitle)) { logs.push(`[cdp] timeout waiting for title '${waitTitle}', got '${title}'`); code = 2; }
  if (opt.input) {
    const m = (type, x, y, extra = {}) => send('Input.dispatchMouseEvent', { type, x, y, ...extra }, s);
    logs.push('[cdp] --- mouse move/press/release');
    await m('mouseMoved', 100, 100); await m('mousePressed', 100, 100, { button: 'left', clickCount: 1 });
    await m('mouseReleased', 100, 100, { button: 'left', clickCount: 1 }); await sleep(100);
    logs.push('[cdp] --- wheel deltaY=120 (pixel)');
    await m('mouseWheel', 100, 100, { deltaX: 0, deltaY: 120 }); await sleep(100);
    logs.push('[cdp] --- ctrl+wheel deltaY=-10 (what a macOS trackpad pinch looks like)');
    await m('mouseWheel', 100, 100, { deltaX: 0, deltaY: -10, modifiers: 2 }); await sleep(100);
    logs.push('[cdp] --- keys: Shift down, a down/up, Shift up');
    const k = (type, key, code, modifiers, text) => send('Input.dispatchKeyEvent', { type, key, code, modifiers, ...(text ? { text } : {}) }, s);
    await k('rawKeyDown', 'Shift', 'ShiftLeft', 8); await k('keyDown', 'A', 'KeyA', 8, 'A');
    await k('keyUp', 'A', 'KeyA', 8); await k('keyUp', 'Shift', 'ShiftLeft', 0); await sleep(100);
    logs.push('[cdp] --- two-finger touch start/move/end');
    const t = (type, pts) => send('Input.dispatchTouchEvent', { type, touchPoints: pts }, s);
    await t('touchStart', [{ x: 200, y: 200, id: 1 }, { x: 300, y: 200, id: 2 }]);
    await t('touchMove', [{ x: 180, y: 200, id: 1 }, { x: 320, y: 200, id: 2 }]);
    await t('touchEnd', []); await sleep(200);
  }
  await sleep(extraMs);
  const shot = await send('Page.captureScreenshot', { format: 'png' }, s);
  writeFileSync(out, Buffer.from(shot.data, 'base64'));
  logs.push(`[cdp] title='${title}' wrote ${out} after ${Date.now() - t0} ms`);
  await send('Browser.close').catch(() => {});
} catch (e) { logs.push(`[cdp] error ${e.message}`); code = 1; }
console.log(logs.join('\n'));
await sleep(300); proc.kill(); rmSync(profile, { recursive: true, force: true });
process.exit(code);
