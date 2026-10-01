import type { Project, ServerInfo, MiddlewareInstance } from '@/types/project'
import type { MiddlewareTemplate } from '@/types/catalog'

export interface ValidationIssue {
  code: string
  level: 'error' | 'warning' | 'info'
  message: string
}

/** 某服务器上对外暴露的宿主端口（含所属实例名） */
export function exposedPortsOf(project: Project, serverId: string) {
  return project.instances
    .filter((i) => i.serverId === serverId)
    .flatMap((i) =>
      i.ports
        .filter((p) => p.expose)
        .map((p) => ({ host: p.host, protocol: p.protocol, instance: i }))
    )
}

/** 构建前校验（对应方案设计 §12 的规则编号） */
export function validateProject(
  project: Project,
  templates: MiddlewareTemplate[]
): ValidationIssue[] {
  const issues: ValidationIssue[] = []
  const tplOf = (id: string) => templates.find((t) => t.id === id)

  // ---- 结构完整性 ----
  if (project.servers.length === 0) {
    issues.push({ code: 'I-100', level: 'info', message: '尚未录入服务器' })
  }
  if (project.instances.length === 0) {
    issues.push({ code: 'I-101', level: 'info', message: '尚未编排中间件实例' })
  }
  for (const inst of project.instances) {
    if (!project.servers.some((s) => s.id === inst.serverId)) {
      issues.push({
        code: 'E-100',
        level: 'error',
        message: `实例「${inst.instanceName}」引用的服务器不存在（可能已被删除）`
      })
    }
    if (!inst.image.trim()) {
      issues.push({ code: 'E-101', level: 'error', message: `实例「${inst.instanceName}」未配置镜像引用` })
    }
  }

  // ---- R1 服务器唯一性 ----
  const seen = new Map<string, ServerInfo>()
  for (const s of project.servers) {
    for (const [field, value] of [
      ['名称', s.name.trim()],
      ['主机名', s.hostname.trim()],
      ['IP', s.ip.trim()]
    ] as const) {
      if (!value) continue
      if (seen.has(`${field}:${value}`)) {
        issues.push({ code: 'E-R1', level: 'error', message: `服务器${field}「${value}」重复` })
      } else {
        seen.set(`${field}:${value}`, s)
      }
    }
  }

  // ---- R2 同机宿主端口冲突 ----
  for (const s of project.servers) {
    const byPort = new Map<number, MiddlewareInstance>()
    for (const inst of project.instances.filter((i) => i.serverId === s.id)) {
      for (const p of inst.ports) {
        if (byPort.has(p.host)) {
          issues.push({
            code: 'E-R2',
            level: 'error',
            message: `服务器「${s.name}」宿主端口 ${p.host} 被「${byPort.get(p.host)!.instanceName}」与「${inst.instanceName}」同时占用`
          })
        } else {
          byPort.set(p.host, inst)
        }
      }
    }
  }

  // ---- R3 架构兼容 / R7 必填参数 / R6 latest ----
  for (const inst of project.instances) {
    const server = project.servers.find((s) => s.id === inst.serverId)
    if (!server) continue
    const tpl = tplOf(inst.templateId)
    if (tpl) {
      if (tpl.supportedArches.length && !tpl.supportedArches.includes(server.arch)) {
        issues.push({
          code: 'E-R3',
          level: 'error',
          message: `「${inst.instanceName}」（${tpl.displayName}）不支持 ${server.arch}（服务器 ${server.name}）`
        })
      }
      for (const h of tpl.envHints) {
        if (h.required && !String(inst.params[h.key] ?? '').trim()) {
          issues.push({
            code: 'E-R7',
            level: 'error',
            message: `「${inst.instanceName}」缺少必填参数：${h.label}（${h.key}）`
          })
        }
      }
    }
    if (inst.image.endsWith(':latest')) {
      issues.push({
        code: 'W-R6',
        level: 'warning',
        message: `「${inst.instanceName}」使用 latest 标签，构建产物不可复现，建议锁定具体版本`
      })
    }
    if (server.arch === 'loongarch64') {
      issues.push({
        code: 'I-R12',
        level: 'info',
        message: `服务器「${server.name}」为 loongarch64 架构，镜像生态需逐个确认（信创提示）`
      })
    }
  }

  // ---- R4 资源估算 ----
  for (const s of project.servers) {
    const need = project.instances
      .filter((i) => i.serverId === s.id)
      .reduce((sum, i) => sum + (tplOf(i.templateId)?.minMemoryGb ?? 0.5), 0)
    if (need > s.memoryGb) {
      issues.push({
        code: 'E-R4',
        level: 'error',
        message: `服务器「${s.name}」内存不足：中间件最低需求合计 ${need.toFixed(1)}GB > ${s.memoryGb}GB`
      })
    } else if (need > s.memoryGb * 0.9) {
      issues.push({
        code: 'W-R4',
        level: 'warning',
        message: `服务器「${s.name}」内存余量偏小：中间件最低需求 ${need.toFixed(1)}GB 已超 ${s.memoryGb}GB 的 90%`
      })
    }
  }

  // ---- R9 访问规则目标端口未暴露 ----
  for (const r of project.networkRules) {
    const toServer = project.servers.find((s) => s.id === r.toServerId)
    if (!toServer) {
      issues.push({ code: 'E-R9a', level: 'error', message: '访问规则的目标服务器不存在' })
      continue
    }
    const exposed = exposedPortsOf(project, r.toServerId)
    if (!exposed.some((p) => p.host === r.toPort)) {
      issues.push({
        code: 'E-R9',
        level: 'error',
        message: `访问规则目标端口未被暴露：→ ${toServer.name}:${r.toPort}`
      })
    }
  }

  // ---- 本地镜像提示 ----
  for (const inst of project.instances) {
    if (!inst.localImageTar && !inst.digest) {
      issues.push({
        code: 'I-102',
        level: 'info',
        message: `「${inst.instanceName}」按镜像引用在线拉取（构建时可自动拉取或从缓存复用）；也可在实例表单指定本地 docker save 的 tar`
      })
    }
  }

  return issues
}

export function hasBlockingErrors(issues: ValidationIssue[]): boolean {
  return issues.some((i) => i.level === 'error')
}
