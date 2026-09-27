// Pixel comparison of two PNGs (no npm deps; Node >= 18).
//
//   node pngdiff.mjs <a.png> <b.png> [--crop-a=x,y,w,h] [--crop-b=x,y,w,h] [--out=diff.png]
//
// Crops (device pixels) select the compared regions; without them the whole images are compared
// (sizes must match). Both are composited on white. Prints:
//   mean   mean absolute difference over all pixels and RGB channels, % of full scale
//   max    largest channel difference (0-255)
//   >8     % of pixels whose largest channel difference exceeds 8
//   >32    % of pixels whose largest channel difference exceeds 32
// --out writes a diff image: white = equal, darker gray = larger difference.
import { readFileSync, writeFileSync } from 'node:fs';
import { inflateSync, deflateSync } from 'node:zlib';

const argv = process.argv.slice(2);
const opt = Object.fromEntries(argv.filter(a => a.startsWith('--')).map(a => a.slice(2).split('=')));
const [fa, fb] = argv.filter(a => !a.startsWith('--'));

/** Decodes an 8-bit, non-interlaced grayscale/RGB/RGBA PNG into RGB composited on white. */
function decode(path) {
  const buf = readFileSync(path);
  let pos = 8, w = 0, h = 0, type = 0, depth = 0, interlace = 0;
  const idat = [];
  while (pos < buf.length) {
    const len = buf.readUInt32BE(pos), kind = buf.toString('latin1', pos + 4, pos + 8);
    const data = buf.subarray(pos + 8, pos + 8 + len);
    if (kind === 'IHDR') {
      w = data.readUInt32BE(0); h = data.readUInt32BE(4);
      depth = data[8]; type = data[9]; interlace = data[12];
    } else if (kind === 'IDAT') idat.push(data);
    else if (kind === 'IEND') break;
    pos += 12 + len;
  }
  const channels = { 0: 1, 2: 3, 4: 2, 6: 4 }[type];
  if (depth !== 8 || !channels || interlace) throw new Error(`${path}: unsupported PNG (type ${type}, depth ${depth})`);
  const raw = inflateSync(Buffer.concat(idat));
  const stride = w * channels, out = new Uint8Array(w * h * 3);
  let prev = new Uint8Array(stride), cur = new Uint8Array(stride);
  for (let y = 0; y < h; y++) {
    const f = raw[y * (stride + 1)], line = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    for (let i = 0; i < stride; i++) {
      const a = i >= channels ? cur[i - channels] : 0, b = prev[i], c = i >= channels ? prev[i - channels] : 0;
      let p;
      switch (f) {
        case 0: p = 0; break;
        case 1: p = a; break;
        case 2: p = b; break;
        case 3: p = (a + b) >> 1; break;
        case 4: { const q = a + b - c, pa = Math.abs(q - a), pb = Math.abs(q - b), pc = Math.abs(q - c);
          p = pa <= pb && pa <= pc ? a : pb <= pc ? b : c; break; }
        default: throw new Error(`${path}: bad filter ${f}`);
      }
      cur[i] = (line[i] + p) & 255;
    }
    for (let x = 0; x < w; x++) {
      const s = x * channels, gray = channels < 3;
      const r = cur[s], g = gray ? r : cur[s + 1], bl = gray ? r : cur[s + 2];
      const al = channels === 4 ? cur[s + 3] : channels === 2 ? cur[s + 1] : 255;
      const o = (y * w + x) * 3, k = al / 255;
      out[o] = Math.round(r * k + 255 * (1 - k));
      out[o + 1] = Math.round(g * k + 255 * (1 - k));
      out[o + 2] = Math.round(bl * k + 255 * (1 - k));
    }
    [prev, cur] = [cur, prev];
  }
  return { w, h, px: out };
}

function crop(img, spec) {
  if (!spec) return img;
  const [x0, y0, w, h] = spec.split(',').map(Number);
  if (x0 + w > img.w || y0 + h > img.h) throw new Error(`crop ${spec} outside ${img.w}x${img.h}`);
  const px = new Uint8Array(w * h * 3);
  for (let y = 0; y < h; y++) px.set(img.px.subarray(((y0 + y) * img.w + x0) * 3, ((y0 + y) * img.w + x0 + w) * 3), y * w * 3);
  return { w, h, px };
}

const crc32 = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; t[n] = c >>> 0; }
  return b => { let c = 0xffffffff; for (const v of b) c = t[(c ^ v) & 255] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
})();

function encodeGray(w, h, px) {
  const raw = Buffer.alloc(h * (w + 1));
  for (let y = 0; y < h; y++) raw.set(px.subarray(y * w, (y + 1) * w), y * (w + 1) + 1);
  const chunk = (kind, data) => {
    const b = Buffer.alloc(12 + data.length);
    b.writeUInt32BE(data.length, 0); b.write(kind, 4, 'latin1'); data.copy(b, 8);
    b.writeUInt32BE(crc32(b.subarray(4, 8 + data.length)), 8 + data.length);
    return b;
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4); ihdr[8] = 8; ihdr[9] = 0;
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw)), chunk('IEND', Buffer.alloc(0))]);
}

const a = crop(decode(fa), opt['crop-a']), b = crop(decode(fb), opt['crop-b']);
if (a.w !== b.w || a.h !== b.h) {
  console.error(`size mismatch: ${a.w}x${a.h} vs ${b.w}x${b.h}`);
  process.exit(2);
}
let sum = 0, max = 0, over8 = 0, over32 = 0;
const diff = new Uint8Array(a.w * a.h);
for (let i = 0; i < a.w * a.h; i++) {
  let m = 0;
  for (let c = 0; c < 3; c++) { const d = Math.abs(a.px[i * 3 + c] - b.px[i * 3 + c]); sum += d; m = Math.max(m, d); }
  max = Math.max(max, m); if (m > 8) over8++; if (m > 32) over32++;
  diff[i] = 255 - Math.min(255, m * 4);
}
const n = a.w * a.h;
console.log(`${a.w}x${a.h}  mean ${(100 * sum / (3 * 255 * n)).toFixed(3)}%  max ${max}  >8 ${(100 * over8 / n).toFixed(3)}%  >32 ${(100 * over32 / n).toFixed(3)}%`);
if (opt.out) writeFileSync(opt.out, encodeGray(a.w, a.h, diff));
