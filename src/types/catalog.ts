// 与 src-tauri/src/catalog.rs 对应

export interface TemplatePort {
  name: string
  container: number
  defaultHost: number
  protocol: string
}

export interface EnvHint {
  key: string
  label: string
  secret: boolean
  required: boolean
  default: string
}

export interface HealthCheck {
  type: 'exec' | 'tcp' | string
  cmd: string[]
}

export interface MiddlewareTemplate {
  id: string
  displayName: string
  category: string
  defaultImage: string
  recommendedTags: string[]
  supportedArches: string[]
  ports: TemplatePort[]
  envHints: EnvHint[]
  dataVolume: string
  healthCheck: HealthCheck | null
  minMemoryGb: number
  kernelReqs: string[]
  remark: string
}

export interface CatalogFile {
  version: number
  updatedAt: string
  templates: MiddlewareTemplate[]
}
