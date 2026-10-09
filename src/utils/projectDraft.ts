import type { MiddlewareInstance, Project } from '@/types/project'
import type { MiddlewareTemplate } from '@/types/catalog'
import { genId } from '@/utils/id'

const equal = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b)
/** Apply the saved draft while preserving edits made during its request. */
export function mergeSavedDraft<T>(before: T, saved: T, current: T): T {
  if (equal(before, current)) return saved
  if (equal(before, saved)) return current
  if (before && saved && current && typeof before === 'object' && typeof saved === 'object' && typeof current === 'object') {
    if (Array.isArray(before) && Array.isArray(saved) && Array.isArray(current)) {
      if ([...before, ...saved, ...current].every(v => v && typeof v.id === 'string')) {
        const old = new Map(before.map(v => [v.id, v]))
        const next = new Map(saved.map(v => [v.id, v]))
        const live = new Map(current.map(v => [v.id, v]))
        for (const [id, v] of next) {
          if (!old.has(id)) { if (!live.has(id)) live.set(id, v) }
          else if (live.has(id)) live.set(id, mergeSavedDraft(old.get(id), v, live.get(id)))
        }
        for (const [id, v] of old) if (!next.has(id) && equal(live.get(id), v)) live.delete(id)
        return [...live.values()] as T
      }
      return current
    }
    if (!Array.isArray(before) && !Array.isArray(saved) && !Array.isArray(current)) {
      const result: Record<string, unknown> = { ...(current as Record<string, unknown>) }
      for (const key of new Set([...Object.keys(before), ...Object.keys(saved)])) {
        result[key] = mergeSavedDraft((before as any)[key], (saved as any)[key], (current as any)[key])
      }
      return result as T
    }
  }
  return current
}

/** Migrate rules by port identity; retired exposure removes only rules with no other owner. */
export function replaceInstance(project: Project, old: MiddlewareInstance, next?: MiddlewareInstance) {
  const index = project.instances.findIndex(i => i.id === old.id)
  if (next) project.instances[index] = next
  else project.instances.splice(index, 1)
  project.networkRules = project.networkRules.flatMap(rule => {
    const port = old.ports.find(p => p.expose && rule.toServerId === old.serverId && rule.toPort === p.host && rule.protocol === p.protocol)
    if (!port) return [rule]
    const stillOwned = project.instances.some(i => i.id !== old.id && i.serverId === rule.toServerId && i.ports.some(p => p.expose && p.host === rule.toPort && p.protocol === rule.protocol))
    if (stillOwned) return [rule]
    const replacements = next?.ports.filter(p => p.name === port.name && p.container === port.container && p.expose) ?? []
    if (replacements.length !== 1) return []
    return [{ ...rule, toServerId: next!.serverId, toPort: replacements[0].host, protocol: replacements[0].protocol }]
  }).filter((rule, index, all) => all.findIndex(r => r.fromServerId === rule.fromServerId && r.toServerId === rule.toServerId && r.toPort === rule.toPort && r.protocol === rule.protocol) === index)
}

/** Instance settings belong to one instance, including when created in a batch. */
export function applyInstanceTemplate(project: Project, instances: MiddlewareInstance[], template?: MiddlewareTemplate): MiddlewareInstance[] {
  if (!template) return instances
  let snapshots = [...(project.templateSnapshots ?? [])]
  const configured = instances.map(inst => {
    const shared = instances.length > 1 || project.instances.some(i => i.id !== inst.id && i.templateId === template.id)
    const id = template.category === '实例配置' && shared ? genId('instcfg') : template.id
    snapshots = [...snapshots.filter(t => t.id !== id), { ...template, id }]
    return { ...inst, templateId: id }
  })
  project.templateSnapshots = snapshots
  return configured
}
