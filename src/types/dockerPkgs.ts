// 与 src-tauri/src/docker_pkgs.rs 对应

export interface DockerPkgEntry {
  osFamily: string
  osVersion: string
  id: string
  arch: string
  kind: 'rpm' | 'deb' | 'static' | string
  dockerVersion: string
  dir: string
  sizeBytes: number
  files: string[]
  importedAt: string
  sources: Record<string, string>
}

export interface ComposePluginStatus {
  arch: string
  installed: boolean
  file: string
  sizeBytes: number
  version: string
  source: string
  error: string | null
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
