import { defineStore } from 'pinia'
import { ref } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { backend } from '@/api/backend'
import type { Project } from '@/types/project'
import type { BuildResult, BuildProgressEvent } from '@/types/build'

export const useBuildTaskStore = defineStore('buildTask', () => {
  const busy = ref(false)
  const records = ref<Record<string, { logs: string[]; result: BuildResult | null }>>({})
  let pending: Promise<BuildResult> | null = null
  function run(project: Project, autoPull: boolean, baseline: string | null, prepare?: () => Promise<void>) {
    if (busy.value) throw new Error('已有构建任务运行')
    busy.value = true
    const snapshot: Project = JSON.parse(JSON.stringify(project))
    const taskId = crypto.randomUUID()
    const record = { logs: [] as string[], result: null as BuildResult | null }
    records.value[snapshot.id] = record
    pending = (async () => {
      let unlisten: (() => void) | undefined
      try {
        await prepare?.()
        unlisten = await listen<BuildProgressEvent>('build-progress', ({ payload }) => {
          if (payload.taskId === taskId && payload.projectId === snapshot.id) {
            records.value[snapshot.id].logs.push(`${payload.step}: ${payload.detail}`)
          }
        })
        const result = await backend.buildOfflinePackage(snapshot, autoPull, baseline, taskId)
        records.value[snapshot.id].result = result
        return result
      } finally {
        unlisten?.()
        busy.value = false
        pending = null
      }
    })()
    return pending
  }
  async function waitForIdle() { await pending?.catch(() => undefined) }
  return { busy, records, run, waitForIdle }
})
