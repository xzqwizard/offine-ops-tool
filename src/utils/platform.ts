export function normalizeArch(arch: string): string {
  return ({ x86_64: 'amd64', aarch64: 'arm64', loong64: 'loongarch64' } as Record<string, string>)[arch.toLowerCase()] ?? arch.toLowerCase()
}
export function platformMatches(platform: string, arch: string): boolean {
  const [os, cpu] = platform.split('/')
  return os === 'linux' && normalizeArch(cpu?.split(':')[0] ?? '') === normalizeArch(arch)
}
export const supportedArch = (arches: string[], arch: string) => !arches.length || arches.some(a => normalizeArch(a) === normalizeArch(arch))
