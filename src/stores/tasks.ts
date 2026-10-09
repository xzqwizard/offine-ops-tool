import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { backend, toAppError } from '@/api/backend'

export interface TaskRecord {
  id: string; key: string; label: string; parameters: Record<string, unknown>
  status: 'running' | 'cancelling' | 'complete' | 'failed' | 'cancelled'
  logs: string[]; result?: unknown; error?: string
  startedAt: number; elapsedMs: number; progress?: { percent: number; bytes: number; total: number; speedBps: number }
  items?: Record<string, any>
}
export const useTaskStore = defineStore('tasks', () => {
  const records = ref<TaskRecord[]>([])
  const active = computed(() => records.value.filter(r => r.status === 'running' || r.status === 'cancelling'))
  const busy = computed(() => active.value.length > 0)
  const closing = ref(false)
  const pending = new Map<string, Promise<unknown>>()
  const latest = (prefix: string) => records.value.find(r => r.key.startsWith(prefix))
  function run<T>(key: string, label: string, parameters: Record<string, unknown>, events: string[], execute: (taskId: string) => Promise<T>, onEvent?: (payload: any) => void): Promise<T> {
    if (closing.value) throw new Error('应用正在关闭，暂不能开始新任务')
    if (active.value.some(r => r.key === key)) throw new Error('该任务正在运行，请等待或取消')
    if (active.value.length >= 2) throw new Error('已有两个任务运行，请等待完成后重试')
    const id = crypto.randomUUID()
    records.value.unshift({ id, key, label, parameters: JSON.parse(JSON.stringify(parameters)), status: 'running', logs: [], startedAt: Date.now(), elapsedMs: 0 })
    const record = records.value[0]
    const work = Promise.resolve().then(async () => {
      const unlisten: (() => void)[] = []
      try {
        for (const name of events) {
          unlisten.push(await listen<any>(name, ({ payload }) => {
            if (payload.taskId !== id) return
            if (typeof payload.percent === 'number') record.progress = payload
            if (payload.target) { record.items ??= {}; record.items[payload.target] = payload }
            if (payload.detail) record.logs.push(`${payload.step ?? payload.status ?? name}: ${payload.detail}`)
            if (record.logs.length > 300) record.logs.splice(0, record.logs.length - 300)
            onEvent?.(payload)
          }))
        }
        const result = await execute(id)
        record.result = result
        record.status = 'complete'
        return result
      } catch (e) {
        record.error = toAppError(e).message
        record.status = record.error.includes('任务已取消') ? 'cancelled' : 'failed'
        record.logs.push(record.error)
        throw e
      } finally {
        unlisten.forEach(fn => fn())
        record.elapsedMs = Date.now() - record.startedAt
        pending.delete(id)
        const activeIds = new Set(active.value.map(r => r.id))
        let historyRoom = Math.max(0, 50 - activeIds.size)
        records.value = records.value.filter(r => activeIds.has(r.id) || historyRoom-- > 0)
      }
    })
    pending.set(id, work)
    return work
  }
  async function cancel(id: string) {
    const record = records.value.find(r => r.id === id)
    if (!record || !['running', 'cancelling'].includes(record.status)) return
    await backend.cancelTask(id)
    if (record.status === 'running') record.status = 'cancelling'
  }
  async function waitForIdle() { await Promise.allSettled([...pending.values()]) }
  async function cancelAll() { await Promise.all(active.value.map(r => cancel(r.id))); await waitForIdle() }
  return { records, active, busy, closing, latest, run, cancel, waitForIdle, cancelAll }
})
