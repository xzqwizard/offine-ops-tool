// 与 src-tauri/src/engine.rs、images.rs 对应

export interface EngineStatus {
  installed: boolean
  version: string | null
  path: string
  lastError: string | null
}

export interface EngineInstallEvent {
  step: string
  detail: string
}

export interface ImageInspect {
  isList: boolean
  arches: string[]
  digest: string
  source: string
}

export interface CachedImage {
  reference: string
  platform: string
  file: string
  sizeBytes: number
  pulledAt: string
}

export interface PullResult {
  reference: string
  platform: string
  cacheFile: string
  sizeBytes: number
  digest: string
  source: string
  cached: boolean
  elapsedMs: number
}

export interface ImagePullEvent {
  status: string
  detail: string
}
