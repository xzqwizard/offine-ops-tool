import type { Project } from '@/types/project'

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

/** The backend owns project validation for GUI, imports and CLI. */
export function hasBlockingErrors(issues: ValidationIssue[]): boolean { return issues.some(i => i.level === 'error') }
export function isIpv4(value: string): boolean {
  const parts=value.split('.')
  return parts.length === 4 && parts.every(p => /^(0|[1-9][0-9]{0,2})$/.test(p) && Number(p) <= 255)
}
