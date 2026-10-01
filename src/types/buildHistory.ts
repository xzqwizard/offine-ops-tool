// 与 src-tauri/src/build_history.rs 对应

export interface BuildHistoryImage {
  reference: string
  digest: string
  file: string
}

export interface BuildHistoryServer {
  name: string
  arch: string
  dirName: string
  packageFile: string | null
  sizeBytes: number
  images: BuildHistoryImage[]
}

export interface BuildHistoryEntry {
  buildId: string
  projectId: string
  projectName: string
  generatedAt: string
  kind: 'full' | 'upgrade' | string
  servers: BuildHistoryServer[]
  dir: string
  totalSizeBytes: number
}
