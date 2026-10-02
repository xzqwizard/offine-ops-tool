/** 自动生成强密码（排除易混淆字符 0O1lI，保证大小写/数字/符号各至少 1 个） */
export function genStrongPassword(length = 16): string {
  const upper = 'ABCDEFGHJKLMNPQRSTUVWXYZ'
  const lower = 'abcdefghijkmnpqrstuvwxyz'
  const digits = '23456789'
  const symbols = '!@#$%^&*-_+='
  const all = upper + lower + digits + symbols
  const len = Math.max(8, Math.min(length, 64))

  const buf = new Uint32Array(len)
  crypto.getRandomValues(buf)
  // 每次取样用独立随机字（同一 buf[0] 派生四类会严格配对，熵坍缩）
  const pick = (idx: number, set: string) => set[buf[idx] % set.length]
  const chars: string[] = [pick(0, upper), pick(1, lower), pick(2, digits), pick(3, symbols)]
  for (let i = chars.length; i < len; i++) {
    chars.push(all[buf[i] % all.length])
  }
  // Fisher-Yates 洗牌（避免前四位固定是四类字符）
  for (let i = chars.length - 1; i > 0; i--) {
    const j = buf[i] % (i + 1)
    ;[chars[i], chars[j]] = [chars[j], chars[i]]
  }
  return chars.join('')
}
