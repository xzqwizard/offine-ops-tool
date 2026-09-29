// 与 src-tauri/src/builder.rs 对应

export interface BuildImageInfo {
  reference: string
  file: string
  sha256: string | null
  sizeBytes: number
}

export interface BuildServerResult {
  name: string
  arch: string
  dirName: string
  packageFile: string | null
  sizeBytes: number
  images: BuildImageInfo[]
  warnings: string[]
}

export interface BuildResult {
  buildId: string
  outputDir: string
  generatedAt: string
  servers: BuildServerResult[]
  warnings: string[]
}

export interface BuildProgressEvent {
  step: string
  detail: string
}
