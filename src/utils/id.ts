/** 生成短 id：prefix-<uuid 简化 32 位> */
export function genId(prefix: string): string {
  const uuid = crypto.randomUUID().replace(/-/g, '')
  return `${prefix}-${uuid}`
}
