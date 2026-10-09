import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { Project } from '@/types/project'
const mocks = vi.hoisted(() => ({ load: vi.fn(), save: vi.fn(), list: vi.fn(), build: vi.fn(), listen: vi.fn() }))
vi.mock('@/api/backend', () => ({ backend: { loadProject: mocks.load, saveProject: mocks.save, listProjects: mocks.list, buildOfflinePackage: mocks.build }, toAppError: (e: unknown) => e instanceof Error ? e : new Error(String(e)) }))
vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }))
import { useProjectStore } from '@/stores/project'
import { useBuildTaskStore } from '@/stores/buildTask'
import { withTag } from '@/utils/imageReference'
import { isIpv4 } from '@/utils/validate'
import { applyInstanceTemplate, replaceInstance } from '@/utils/projectDraft'
import type { MiddlewareTemplate } from '@/types/catalog'
import { platformMatches } from '@/utils/platform'
function project(id: string): Project { return { schemaVersion: 1, id, name: id, customer: '', createdAt: '', updatedAt: '', buildConfig: { packageFormat: 'dir', recompressImages: false, sign: false }, servers: [], instances: [], networkRules: [], registry: null } }
function deferred<T>() { let resolve!: (v: T) => void; const promise = new Promise<T>(r => { resolve = r }); return { promise, resolve } }
beforeEach(() => {
  setActivePinia(createPinia())
  vi.clearAllMocks()
  vi.stubGlobal('window', { setTimeout, clearTimeout })
  mocks.list.mockResolvedValue([])
  mocks.save.mockImplementation(async p => p)
  mocks.listen.mockResolvedValue(() => undefined)
})
describe('project persistence', () => {
  it('keeps the latest project when loads finish in reverse order', async () => {
    const a = deferred<Project>(), b = deferred<Project>()
    mocks.load.mockImplementation((id: string) => id === 'a' ? a.promise : b.promise)
    const store = useProjectStore()
    const first = store.open('a')
    await vi.waitFor(() => expect(mocks.load).toHaveBeenCalledWith('a'))
    const second = store.open('b')
    await vi.waitFor(() => expect(mocks.load).toHaveBeenCalledWith('b'))
    b.resolve(project('b')); await second
    a.resolve(project('a')); await first
    expect(store.project?.id).toBe('b')
  })
  it('never writes edits into an in-flight save snapshot, and flushes the newer revision', async () => {
    const store = useProjectStore(); store.project = project('a')
    const saved = deferred<Project>(); mocks.save.mockReturnValueOnce(saved.promise)
    const first = store.commit(p => { p.name = 'first' })
    await vi.waitFor(() => expect(mocks.save).toHaveBeenCalledTimes(1))
    store.scheduleSave(p => { p.name = 'latest' }, 100000)
    expect(mocks.save.mock.calls[0][0].name).toBe('first')
    saved.resolve(project('a')); await first
    expect(store.project.name).toBe('latest')
    await store.flushPending()
    expect(mocks.save.mock.calls[mocks.save.mock.calls.length - 1]?.[0].name).toBe('latest')
    expect(store.dirty).toBe(false)
  })
  it('keeps dirty state and refuses close when saving fails', async () => {
    const store = useProjectStore(); store.project = project('a')
    mocks.save.mockRejectedValue(new Error('disk full'))
    store.scheduleSave(p => { p.name = 'unsaved' }, 100000)
    await expect(store.close()).rejects.toThrow('disk full')
    expect(store.project?.name).toBe('unsaved'); expect(store.dirty).toBe(true)
    mocks.save.mockImplementation(async p => p)
    await store.close(); expect(store.project).toBeNull()
  })
  it('commits discrete edits only after persistence succeeds, leaving a rejected draft retryable', async () => {
    const store = useProjectStore(); store.project = project('a')
    mocks.save.mockRejectedValueOnce(new Error('validation failed'))
    await expect(store.commit(p => { p.name = 'draft' })).rejects.toThrow('validation failed')
    expect(store.project.name).toBe('a'); expect(store.dirty).toBe(false)
    await store.commit(p => { p.name = 'retry' })
    expect(store.project.name).toBe('retry')
    await store.close(); expect(store.project).toBeNull()
  })
  it('merges the successful discrete edit with newer continuous input', async () => {
    const store = useProjectStore(); store.project = project('a')
    const pending = deferred<Project>(); mocks.save.mockReturnValueOnce(pending.promise)
    const edit = store.commit(p => { p.customer = 'new customer' })
    await vi.waitFor(() => expect(mocks.save).toHaveBeenCalledTimes(1))
    store.scheduleSave(p => { p.name = 'newer name' }, 100000)
    pending.resolve({ ...project('a'), customer: 'new customer' }); await edit
    expect(store.project.customer).toBe('new customer')
    expect(store.project.name).toBe('newer name')
    await store.flushPending()
    expect(mocks.save.mock.calls[mocks.save.mock.calls.length - 1]?.[0].customer).toBe('new customer')
  })
})
it('migrates rules for edited ports and protocols and removes retired exposure', () => {
  const p = project('a')
  const old = { id: 'i', serverId: 's', templateId: 'custom', image: 'demo:v1', digest: '', instanceName: 'demo', params: {}, localImageTar: '', ports: [{ name: 'api', host: 8080, container: 80, protocol: 'tcp', expose: true }] }
  p.instances = [old]
  p.networkRules = [{ id: 'r', fromServerId: 'client', toServerId: 's', toPort: 8080, protocol: 'tcp', description: '' }]
  const next = { ...old, serverId: 'other', ports: [{ ...old.ports[0], host: 9080, protocol: 'udp' }] }
  replaceInstance(p, old, next)
  expect(p.networkRules[0]).toMatchObject({ toServerId: 'other', toPort: 9080, protocol: 'udp' })
  replaceInstance(p, next, { ...next, ports: [{ ...next.ports[0], expose: false }] })
  expect(p.networkRules).toEqual([])
})
it('matches architecture aliases and Linux variants without accepting attestation/32 bit ARM', () => {
  expect(platformMatches('linux/loong64', 'loongarch64')).toBe(true)
  expect(platformMatches('linux/aarch64/v8', 'arm64')).toBe(true)
  expect(platformMatches('linux/x86_64', 'amd64')).toBe(true)
  expect(platformMatches('linux/arm:v7', 'arm64')).toBe(false)
  expect(platformMatches('unknown/unknown', 'amd64')).toBe(false)
})
it('isolates batch instance settings and detaches shared settings without changing peers', () => {
  const p = project('a')
  const template: MiddlewareTemplate = { id: 'instcfg-shared', displayName: 'demo', category: '实例配置', defaultImage: 'demo', recommendedTags: [], supportedArches: [], ports: [], envHints: [], dataVolume: '/data', dataUser: null, command: [], healthCheck: null, healthTimeoutSec: 60, minMemoryGb: 0, kernelReqs: [], remark: '' }
  const a = { id: 'i-a', serverId: 's-a', templateId: template.id, image: 'demo:v1', digest: '', instanceName: 'demo', params: {}, localImageTar: '', ports: [] }
  const b = { ...a, id: 'i-b', serverId: 's-b' }
  const batch = applyInstanceTemplate(p, [a, b], template)
  expect(batch[0].templateId).not.toBe(batch[1].templateId)
  expect(p.templateSnapshots?.map(t => t.id)).toEqual(batch.map(i => i.templateId))

  p.instances = [a, b]; p.templateSnapshots = [template]
  const edited = applyInstanceTemplate(p, [a], { ...template, dataVolume: '/new-data', command: ['serve'] })[0]
  expect(edited.templateId).not.toBe(template.id)
  expect(p.instances[1].templateId).toBe(template.id)
  expect(p.templateSnapshots.find(t => t.id === template.id)).toEqual(template)
  expect(p.templateSnapshots.find(t => t.id === edited.templateId)?.dataVolume).toBe('/new-data')
  p.instances[0] = edited
  expect(applyInstanceTemplate(p, [edited], { ...template, id: edited.templateId })[0].templateId).toBe(edited.templateId)
})
it('runs one immutable build and filters progress by task and project', async () => {
  const pending = deferred<any>(); mocks.build.mockReturnValue(pending.promise)
  let receiver!: (event: any) => void
  const unsubscribe = vi.fn()
  mocks.listen.mockImplementation(async (_name, fn) => { receiver = fn; return unsubscribe })
  const store = useBuildTaskStore(); const p = project('a')
  const run = store.run(p, false, null)
  p.name = 'modified later'
  expect(() => store.run(p, true, null)).toThrow('已有构建')
  await vi.waitFor(() => expect(mocks.build).toHaveBeenCalledTimes(1))
  const taskId = mocks.build.mock.calls[0][3]
  receiver({ payload: { taskId: 'other', projectId: 'a', step: 'start', detail: 'foreign' } })
  receiver({ payload: { taskId, projectId: 'a', step: 'start', detail: 'own' } })
  expect(store.records.a.logs).toEqual(['start: own'])
  expect(mocks.build.mock.calls[0][0].name).toBe('a')
  const result = { buildId: 'b-test' }; pending.resolve(result); await run
  expect(store.records.a.result).toEqual(result); expect(store.busy).toBe(false)
  expect(unsubscribe).toHaveBeenCalledOnce()
})
it('handles digest references and strict IPv4 input', () => {
  const digest = `example.org:5000/app@sha256:${'a'.repeat(64)}`
  expect(withTag(digest, 'latest')).toBe(digest)
  expect(withTag('example.org:5000/app:old', 'new')).toBe('example.org:5000/app:new')
  expect(isIpv4('999.2.3.4')).toBe(false); expect(isIpv4('10.2.3.4')).toBe(true)
})
