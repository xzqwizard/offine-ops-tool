import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { Project } from '@/types/project'
const mocks = vi.hoisted(() => ({ load: vi.fn(), save: vi.fn(), list: vi.fn(), build: vi.fn(), listen: vi.fn() }))
vi.mock('@/api/backend', () => ({ backend: { loadProject: mocks.load, saveProject: mocks.save, listProjects: mocks.list, buildOfflinePackage: mocks.build } }))
vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }))
import { useProjectStore } from '@/stores/project'
import { useBuildTaskStore } from '@/stores/buildTask'
import { withTag } from '@/utils/imageReference'
import { isIpv4 } from '@/utils/validate'
function project(id: string): Project { return { schemaVersion: 1, id, name: id, customer: '', createdAt: '', updatedAt: '', buildConfig: { packageFormat: 'dir', recompressImages: false, sign: false }, servers: [], instances: [], networkRules: [], registry: null } }
function deferred<T>() { let resolve!: (v: T) => void; const promise = new Promise<T>(r => { resolve = r }); return { promise, resolve } }
beforeEach(() => {
  setActivePinia(createPinia())
  vi.clearAllMocks()
  vi.stubGlobal('window', { setTimeout, clearTimeout })
  mocks.list.mockResolvedValue([])
  mocks.save.mockImplementation(async p => p)
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
