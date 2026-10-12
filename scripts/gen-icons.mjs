// 纯 Node 实现的图标源文件生成器 —— 不依赖任何第三方图形库。
// 产出 assets/icon-source.png (1024x1024)，随后由 `pnpm tauri icon` 生成全套平台图标。
//
// 设计：与 docs/design.md 同一套语言 —— 严格单色。
// 近黑圆角方块 + 纯白图钉。图钉是这套界面的签名符号（置顶 = 抬升），图标复用同一意象。

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

/* ------------------------------ 绘制 ------------------------------ */

const SIZE = 1024;
const SS = 4; // 每轴 4x 超采样 → 16 样本/像素，边缘平滑

// 与 src/styles/tokens.css 完全一致的取值
const INK = [0x0a, 0x0a, 0x0a]; // --ink
const PAPER = [0xff, 0xff, 0xff]; // --paper

const PAD = 48;
const RADIUS = 204;

// 图钉几何：顶部法兰 + 收窄的柱身 + 针 + 针尖
// 柱身刻意做矮、收窄幅度刻意做小 —— 否则整体会读成漏斗而非图钉。
const CX = SIZE / 2;
const FLANGE = { left: 318, top: 250, right: 706, bottom: 330, r: 40 };
const BODY = { top: 330, bottom: 566, topHalfW: 144, botHalfW: 68 };
const NEEDLE = { top: 566, bottom: 802, halfW: 23 };
const TIP = { top: 802, bottom: 884 };

function insidePin(x, y) {
  if (
    insideRoundRect(x, y, FLANGE.left, FLANGE.top, FLANGE.right, FLANGE.bottom, FLANGE.r)
  ) {
    return true;
  }
  if (y >= BODY.top && y <= BODY.bottom) {
    const t = (y - BODY.top) / (BODY.bottom - BODY.top);
    const halfW = BODY.topHalfW + (BODY.botHalfW - BODY.topHalfW) * t;
    if (Math.abs(x - CX) <= halfW) return true;
  }
  if (y >= NEEDLE.top && y <= NEEDLE.bottom && Math.abs(x - CX) <= NEEDLE.halfW) {
    return true;
  }
  if (y > TIP.top && y <= TIP.bottom) {
    const t = (y - TIP.top) / (TIP.bottom - TIP.top);
    if (Math.abs(x - CX) <= NEEDLE.halfW * (1 - t)) return true;
  }
  return false;
}

/** 返回某个连续坐标点的 RGBA（0-255，非预乘） */
function sample(x, y) {
  if (!insideRoundRect(x, y, PAD, PAD, SIZE - PAD, SIZE - PAD, RADIUS)) {
    return [0, 0, 0, 0];
  }
  const col = insidePin(x, y) ? PAPER : INK;
  return [col[0], col[1], col[2], 255];
}

function render() {
  const rgba = Buffer.alloc(SIZE * SIZE * 4);
  const inv = 1 / (SS * SS);
  const step = 1 / SS;

  for (let py = 0; py < SIZE; py++) {
    for (let px = 0; px < SIZE; px++) {
      let r = 0,
        g = 0,
        b = 0,
        a = 0;
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
        rgba[off] = 0;
        rgba[off + 1] = 0;
        rgba[off + 2] = 0;
        rgba[off + 3] = 0;
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
