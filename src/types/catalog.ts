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
  /** 数据卷属主（非 root 镜像必须，如 "1000:1000"） */
  dataUser: string | null
  /** compose command 覆盖（如 redis 官方镜像设置密码） */
  dependsOn?: string[]
  command: string[]
  healthCheck: HealthCheck | null
  /** 健康检查超时秒数（默认 60） */
  healthTimeoutSec: number | null
  minMemoryGb: number
  kernelReqs: string[]
  remark: string
}

export interface CatalogFile {
  version: number
  updatedAt: string
  templates: MiddlewareTemplate[]
}
