// 与 src-tauri/src/docker_pkgs.rs 对应

export interface DockerPkgEntry {
  id: string
  arch: string
  kind: 'rpm' | 'deb' | 'static' | string
  dockerVersion: string
  dir: string
  sizeBytes: number
  files: string[]
  importedAt: string
}

export interface ComposePluginStatus {
  arch: string
  installed: boolean
  file: string
  sizeBytes: number
}

export interface ImportResult {
  createdDirs: string[]
  composeInstalled: string[]
  skipped: string[]
}

export interface DockerPkgDownloadEvent {
  step: string
  detail: string
}
