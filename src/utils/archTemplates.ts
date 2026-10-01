import type { Project, ServerInfo, MiddlewareInstance } from '@/types/project'
import { genId } from '@/utils/id'
import { genStrongPassword } from '@/utils/password'
import type { MiddlewareTemplate } from '@/types/catalog'

/** 架构模板：一键生成"服务器 + 中间件实例"骨架（IP 留空待填，密码自动生成） */

export interface ArchTemplateServer {
  name: string
  arch: string
  osFamily: string
  osVersion: string
  memoryGb: number
  diskDataGb: number
}

export interface ArchTemplateInstance {
  /** 对应 servers 数组下标 */
  serverIdx: number
  templateId: string
  /** 需要自动生成强密码的参数 key */
  secretKeys: string[]
}

export interface ArchTemplate {
  id: string
  name: string
  desc: string
  servers: ArchTemplateServer[]
  instances: ArchTemplateInstance[]
}

export const ARCH_TEMPLATES: ArchTemplate[] = [
  {
    id: 'classic',
    name: '经典四件套（双机）',
    desc: '应用服务器（nginx+redis）+ 数据库服务器（mysql+minio），x86 通用',
    servers: [
      { name: '应用服务器', arch: 'amd64', osFamily: 'kylin', osVersion: 'V10 SP3', memoryGb: 16, diskDataGb: 200 },
      { name: '数据库服务器', arch: 'amd64', osFamily: 'kylin', osVersion: 'V10 SP3', memoryGb: 32, diskDataGb: 500 }
    ],
    instances: [
      { serverIdx: 0, templateId: 'nginx-1.27', secretKeys: [] },
      { serverIdx: 0, templateId: 'redis-7', secretKeys: ['REDIS_PASSWORD'] },
      { serverIdx: 1, templateId: 'mysql-8.0', secretKeys: ['MYSQL_ROOT_PASSWORD'] },
      { serverIdx: 1, templateId: 'minio', secretKeys: ['MINIO_ROOT_USER', 'MINIO_ROOT_PASSWORD'] }
    ]
  },
  {
    id: 'xinchuang-single',
    name: '信创单机版',
    desc: '单台鲲鹏/飞腾（arm64 麒麟）：nginx+redis+mysql+minio 全套',
    servers: [
      { name: '信创应用服务器', arch: 'arm64', osFamily: 'kylin', osVersion: 'V10 SP3', memoryGb: 32, diskDataGb: 500 }
    ],
    instances: [
      { serverIdx: 0, templateId: 'nginx-1.27', secretKeys: [] },
      { serverIdx: 0, templateId: 'redis-7', secretKeys: ['REDIS_PASSWORD'] },
      { serverIdx: 0, templateId: 'mysql-8.0', secretKeys: ['MYSQL_ROOT_PASSWORD'] },
      { serverIdx: 0, templateId: 'minio', secretKeys: ['MINIO_ROOT_USER', 'MINIO_ROOT_PASSWORD'] }
    ]
  }
]

/** 按模板生成完整方案内容（叠放到已创建的空方案上） */
export function buildProjectFromTemplate(
  project: Project,
  tpl: ArchTemplate,
  catalog: MiddlewareTemplate[]
): void {
  const serverIds: string[] = []
  for (const s of tpl.servers) {
    const id = genId('srv')
    serverIds.push(id)
    project.servers.push({
      id,
      name: s.name,
      hostname: '',
      ip: '',
      osFamily: s.osFamily,
      osVersion: s.osVersion,
      arch: s.arch,
      bits: 64,
      cpuCores: 8,
      memoryGb: s.memoryGb,
      diskSystemGb: 100,
      diskDataGb: s.diskDataGb,
      dockerVersion: '27.5.1',
      dockerDataRoot: '/data/docker',
      deployBaseDir: '/opt/stack'
    } as ServerInfo)
  }
  for (const i of tpl.instances) {
    const meta = catalog.find((t) => t.id === i.templateId)
    if (!meta) continue
    const params: Record<string, unknown> = {}
    for (const h of meta.envHints) {
      if (h.default) params[h.key] = h.default
    }
    for (const k of i.secretKeys) {
      // MINIO_ROOT_USER 是账号不是密码：用户名用随机可读串，密码用强密码
      params[k] = k.endsWith('USER') ? `admin${genStrongPassword(6)}` : genStrongPassword()
    }
    project.instances.push({
      id: genId('inst'),
      serverId: serverIds[i.serverIdx],
      templateId: i.templateId,
      image: `${meta.defaultImage}:${meta.recommendedTags[0] ?? 'latest'}`,
      digest: '',
      instanceName: meta.id,
      params,
      ports: meta.ports.map((p) => ({
        name: p.name,
        host: p.defaultHost,
        container: p.container,
        protocol: p.protocol,
        expose: true
      })),
      localImageTar: ''
    } as MiddlewareInstance)
  }
}
