// 生成 512x512 应用图标 PNG（无外部依赖，纯 Node zlib 手写 PNG）
// 背景：圆角深色方块（垂直渐变）；中心：浅色圆角方块 + 上提的"箱盖"，表达"离线交付包"
import { deflateSync } from 'node:zlib'
import { writeFileSync, mkdirSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const SIZE = 512
const ROOT = dirname(dirname(fileURLToPath(import.meta.url)))

// 简单 AABB 内圆角矩形判定
function inRoundedRect(x, y, x0, y0, x1, y1, r) {
  if (x < x0 || x > x1 || y < y0 || y > y1) return false
  const cx = Math.max(x0 + r, Math.min(x, x1 - r))
  const cy = Math.max(y0 + r, Math.min(y, y1 - r))
  const dx = x - cx
  const dy = y - cy
  return dx * dx + dy * dy <= r * r + 0.5 // 半像素容差抗锯齿边缘
}

const px = new Uint8Array(SIZE * SIZE * 4)

for (let y = 0; y < SIZE; y++) {
  for (let x = 0; x < SIZE; x++) {
    const i = (y * SIZE + x) * 4
    let r = 11, g = 17, b = 32 // #0b1120 背景（透明区外的兜底色）
    let a = 0

    if (inRoundedRect(x, y, 16, 16, 496, 496, 96)) {
      // 背景：#0f172a → #1e2d48 垂直渐变
      const t = y / SIZE
      r = Math.round(15 + t * 15)
      g = Math.round(23 + t * 22)
      b = Math.round(42 + t * 42)
      a = 255
    }

    // 中心主体：箱子（下半）+ 掀起的箱盖（上半），浅蓝 #38bdf8 → #0ea5e9
    const boxTop = inRoundedRect(x, y, 156, 236, 356, 356, 20)
    const lidTop = inRoundedRect(x, y, 146, 176, 366, 236, 20)
    if (boxTop || lidTop) {
      const t = Math.min(1, Math.max(0, (y - 150) / 210))
      r = Math.round(56 - t * 10)
      g = Math.round(189 - t * 30)
      b = Math.round(248 - t * 10)
    }

    // 箱盖与箱体之间的提手缺口
    if (inRoundedRect(x, y, 226, 236, 286, 260, 8)) {
      r = 15; g = 23; b = 42
    }

    px[i] = r
    px[i + 1] = g
    px[i + 2] = b
    px[i + 3] = a
  }
}

// 组装 PNG
function chunk(type, data) {
  const len = Buffer.alloc(4)
  len.writeUInt32BE(data.length)
  const body = Buffer.concat([Buffer.from(type, 'ascii'), data])
  const crcTable = []
  for (let n = 0; n < 256; n++) {
    let c = n
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1
    crcTable[n] = c >>> 0
  }
  let crc = 0xffffffff
  for (const byte of body) crc = crcTable[(crc ^ byte) & 0xff] ^ (crc >>> 8)
  const crcBuf = Buffer.alloc(4)
  crcBuf.writeUInt32BE((crc ^ 0xffffffff) >>> 0)
  return Buffer.concat([len, body, crcBuf])
}

const ihdr = Buffer.alloc(13)
ihdr.writeUInt32BE(SIZE, 0)
ihdr.writeUInt32BE(SIZE, 4)
ihdr[8] = 8 // bit depth
ihdr[9] = 6 // RGBA
// 每行前加 filter byte 0
const raw = Buffer.alloc(SIZE * (SIZE * 4 + 1))
for (let y = 0; y < SIZE; y++) {
  raw[y * (SIZE * 4 + 1)] = 0
  Buffer.from(px.buffer, y * SIZE * 4, SIZE * 4).copy(raw, y * (SIZE * 4 + 1) + 1)
}
const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk('IHDR', ihdr),
  chunk('IDAT', deflateSync(raw, { level: 9 })),
  chunk('IEND', Buffer.alloc(0))
])

const out = resolve(ROOT, 'scripts/app-icon.png')
mkdirSync(dirname(out), { recursive: true })
writeFileSync(out, png)
console.log('icon written:', out, png.length, 'bytes')
