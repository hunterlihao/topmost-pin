// 纯 Node 实现的图标源文件生成器 —— 不依赖任何第三方图形库。
// 产出 assets/icon-source.png (1024x1024)，随后由 `pnpm tauri icon` 生成全套平台图标。
//
// 设计：圆角方形渐变底 + 白色月牙 + 三颗周期圆点（暗示周期推进）

import { deflateSync } from "node:zlib";
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(__dirname, "..");

/* ------------------------------ PNG 编码 ------------------------------ */

const CRC_TABLE = (() => {
  const t = new Int32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c;
  }
  return t;
})();

function crc32(buf) {
  let c = 0xffffffff;
  for (let i = 0; i < buf.length; i++) c = CRC_TABLE[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length, 0);
  const typeBuf = Buffer.from(type, "latin1");
  const crcBuf = Buffer.alloc(4);
  crcBuf.writeUInt32BE(crc32(Buffer.concat([typeBuf, data])), 0);
  return Buffer.concat([len, typeBuf, data, crcBuf]);
}

function encodePNG(width, height, rgba) {
  const sig = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // color type: RGBA
  ihdr[10] = 0; // deflate
  ihdr[11] = 0; // adaptive filtering
  ihdr[12] = 0; // no interlace

  const stride = width * 4;
  const raw = Buffer.alloc((stride + 1) * height);
  for (let y = 0; y < height; y++) {
    raw[y * (stride + 1)] = 0; // filter type 0 (None)
    rgba.copy(raw, y * (stride + 1) + 1, y * stride, y * stride + stride);
  }

  return Buffer.concat([
    sig,
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

/* ------------------------------ 几何工具 ------------------------------ */

function insideRoundRect(x, y, left, top, right, bottom, r) {
  if (x < left || x > right || y < top || y > bottom) return false;
  const cx = Math.min(Math.max(x, left + r), right - r);
  const cy = Math.min(Math.max(y, top + r), bottom - r);
  const dx = x - cx;
  const dy = y - cy;
  return dx * dx + dy * dy <= r * r;
}

function insideCircle(x, y, cx, cy, r) {
  const dx = x - cx;
  const dy = y - cy;
  return dx * dx + dy * dy <= r * r;
}

const lerp = (a, b, t) => a + (b - a) * t;
const mixRGB = (c1, c2, t) => [
  lerp(c1[0], c2[0], t),
  lerp(c1[1], c2[1], t),
  lerp(c1[2], c2[2], t),
];

/* ------------------------------ 绘制 ------------------------------ */

const SIZE = 1024;
const SS = 4; // 每轴 4x 超采样 → 16 样本/像素，获得平滑边缘

const PAD = 56;
const RADIUS = 232;
const C_TOP = [244, 114, 182]; // #F472B6
const C_BOT = [147, 51, 234]; // #9333EA
const WHITE = [255, 255, 255];

// 月牙 = 大圆 减去 偏移圆
const MOON_BIG = { cx: 0.5 * SIZE, cy: 0.47 * SIZE, r: 0.265 * SIZE };
const MOON_SUB = { cx: 0.605 * SIZE, cy: 0.405 * SIZE, r: 0.243 * SIZE };

// 三颗周期圆点
const DOTS = [
  { cx: 0.355 * SIZE, cy: 0.78 * SIZE, r: 0.03 * SIZE, a: 1.0 },
  { cx: 0.5 * SIZE, cy: 0.78 * SIZE, r: 0.03 * SIZE, a: 0.62 },
  { cx: 0.645 * SIZE, cy: 0.78 * SIZE, r: 0.03 * SIZE, a: 0.32 },
];

/** 返回某个连续坐标点的 RGBA（0-255，非预乘） */
function sample(x, y) {
  if (!insideRoundRect(x, y, PAD, PAD, SIZE - PAD, SIZE - PAD, RADIUS)) {
    return [0, 0, 0, 0];
  }

  const t = (x + y) / (2 * SIZE);
  let col = mixRGB(C_TOP, C_BOT, t);

  if (insideCircle(x, y, MOON_BIG.cx, MOON_BIG.cy, MOON_BIG.r) &&
      !insideCircle(x, y, MOON_SUB.cx, MOON_SUB.cy, MOON_SUB.r)) {
    col = WHITE;
  } else {
    for (const d of DOTS) {
      if (insideCircle(x, y, d.cx, d.cy, d.r)) {
        col = mixRGB(col, WHITE, d.a);
        break;
      }
    }
  }

  return [col[0], col[1], col[2], 255];
}

function render() {
  const rgba = Buffer.alloc(SIZE * SIZE * 4);
  const inv = 1 / (SS * SS);
  const step = 1 / SS;

  for (let py = 0; py < SIZE; py++) {
    for (let px = 0; px < SIZE; px++) {
      let r = 0, g = 0, b = 0, a = 0;
      for (let sy = 0; sy < SS; sy++) {
        for (let sx = 0; sx < SS; sx++) {
          const c = sample(px + (sx + 0.5) * step, py + (sy + 0.5) * step);
          // 以 alpha 加权累加，避免边缘出现黑边
          const w = c[3] / 255;
          r += c[0] * w;
          g += c[1] * w;
          b += c[2] * w;
          a += c[3];
        }
      }
      const alpha = a * inv;
      const off = (py * SIZE + px) * 4;
      if (alpha <= 0.5) {
        rgba[off] = 0; rgba[off + 1] = 0; rgba[off + 2] = 0; rgba[off + 3] = 0;
      } else {
        // 还原为直通 alpha
        const wsum = a / 255;
        rgba[off] = Math.round(r / wsum);
        rgba[off + 1] = Math.round(g / wsum);
        rgba[off + 2] = Math.round(b / wsum);
        rgba[off + 3] = Math.round(alpha);
      }
    }
  }
  return rgba;
}

/* ------------------------------ 入口 ------------------------------ */

const outDir = resolve(ROOT, "assets");
mkdirSync(outDir, { recursive: true });

const png = encodePNG(SIZE, SIZE, render());
const outFile = resolve(outDir, "icon-source.png");
writeFileSync(outFile, png);

console.log(`已生成 ${outFile}  (${SIZE}x${SIZE}, ${(png.length / 1024).toFixed(1)} KB)`);
console.log("下一步：pnpm tauri icon assets/icon-source.png");
