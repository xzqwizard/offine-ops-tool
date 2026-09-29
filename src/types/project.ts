// 与 src-tauri/src/models.rs 的 serde camelCase 输出一一对应

export interface BuildConfig {
  packageFormat: 'tar.gz' | 'zip' | 'tar' | 'dir' | string
  recompressImages: boolean
  sign: boolean
}

export interface PortBinding {
  name: string
  host: number
  container: number
  protocol: string
  expose: boolean
}

export interface ServerInfo {
  id: string
  name: string
  hostname: string
  ip: string
  osFamily: string
  osVersion: string
  arch: Arch
  bits: number
  cpuCores: number
  memoryGb: number
  diskSystemGb: number
  diskDataGb: number
  dockerVersion: string
  dockerDataRoot: string
  deployBaseDir: string
}

export type Arch = 'amd64' | 'arm64' | 'loongarch64' | 'mips64el' | 'sw64' | string

export interface MiddlewareInstance {
  id: string
  serverId: string
  templateId: string
  image: string
  digest: string
  instanceName: string
  params: Record<string, unknown>
  ports: PortBinding[]
  localImageTar: string
}

export interface NetworkRule {
  id: string
  fromServerId: string
  toServerId: string
  toPort: number
  protocol: string
  description: string
}

export interface Project {
  schemaVersion: number
  id: string
  name: string
  customer: string
  createdAt: string
  updatedAt: string
  buildConfig: BuildConfig
  servers: ServerInfo[]
  instances: MiddlewareInstance[]
  networkRules: NetworkRule[]
}

export interface ProjectSummary {
  id: string
  name: string
  customer: string
  updatedAt: string
  serverCount: number
  instanceCount: number
}

export interface AppSettings {
  schemaVersion: number
  projectsRoot: string | null
  imageCacheRoot: string | null
  dockerPkgRoot: string | null
  artifactRoot: string | null
  logRoot: string | null
  registryMirrors: string[]
}

export interface StorageInfo {
  projectsRoot: string
  imageCacheRoot: string
  dockerPkgRoot: string
  artifactRoot: string
  logRoot: string
  configDir: string
}

/** 后端 AppError 序列化形态 */
export interface BackendError {
  kind: 'io' | 'serialize' | 'notFound' | 'invalid' | string
  message: string
}

// ==================== 常量（与设计文档附录 C 对齐） ====================

export const ARCH_OPTIONS: { value: Arch; label: string; hint: string }[] = [
  { value: 'amd64', label: 'x86_64 (amd64)', hint: 'Intel/AMD/海光/兆芯' },
  { value: 'arm64', label: 'aarch64 (arm64)', hint: '鲲鹏/飞腾' },
  { value: 'loongarch64', label: 'loongarch64', hint: '龙芯 3A5000+' },
  { value: 'mips64el', label: 'mips64el', hint: '旧龙芯（仅识别）' },
  { value: 'sw64', label: 'sw64', hint: '申威（仅识别）' }
]

export const OS_FAMILY_OPTIONS = [
  { value: 'kylin', label: '银河麒麟' },
  { value: 'uos', label: '统信 UOS' },
  { value: 'openeuler', label: 'openEuler' },
  { value: 'centos', label: 'CentOS' },
  { value: 'rhel', label: 'RHEL' },
  { value: 'rocky', label: 'Rocky Linux' },
  { value: 'ubuntu', label: 'Ubuntu' },
  { value: 'debian', label: 'Debian' },
  { value: 'deepin', label: 'Deepin' },
  { value: 'neokylin', label: '中标麒麟' }
]
