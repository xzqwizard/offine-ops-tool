// 与 src-tauri/src/build_history.rs 对应

export interface BuildHistoryImage {
  reference: string
  digest: string
  file: string
}

export interface BuildHistoryServer {
  serverId: string
  osFamily: string
  osVersion: string
  dockerVersion: string
  dockerDataRoot: string
  deployBaseDir: string
  ip: string
  changedImageCount: number
  name: string
  arch: string
  dirName: string
  packageFile: string | null
  sizeBytes: number
  images: BuildHistoryImage[]
}

export interface BuildHistoryEntry {
  schemaVersion: number
  status: string
  buildId: string
  projectId: string
  projectName: string
  generatedAt: string
  kind: 'full' | 'upgrade' | string
  servers: BuildHistoryServer[]
  dir: string
  totalSizeBytes: number
  hasSnapshot?: boolean
}
