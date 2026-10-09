import { defineStore } from 'pinia'
import { ref } from 'vue'
import { useTaskStore } from '@/stores/tasks'
import { backend } from '@/api/backend'
import type { Project } from '@/types/project'
import type { BuildResult } from '@/types/build'

export const useBuildTaskStore = defineStore('buildTask', () => {
  const busy = ref(false)
  const records = ref<Record<string, { logs: string[]; result: BuildResult | null }>>({})
  let pending: Promise<BuildResult> | null = null
  function run(project: Project, autoPull: boolean, baseline: string | null, prepare?: () => Promise<void>) {
    if (busy.value) throw new Error('已有构建任务运行')
    busy.value = true
    const snapshot: Project = JSON.parse(JSON.stringify(project))
    const record = { logs: [] as string[], result: null as BuildResult | null }
    records.value[snapshot.id] = record
    pending = (async () => {
      try {
        const result = await useTaskStore().run('build', `构建：${snapshot.name}`, { projectId: snapshot.id, baseline }, ['build-progress'], async taskId => {
          await prepare?.()
          return backend.buildOfflinePackage(snapshot, autoPull, baseline, taskId)
        }, payload => {
          if (payload.projectId === snapshot.id) {
            records.value[snapshot.id].logs.push(`${payload.step}: ${payload.detail}`)
          }
        })
        records.value[snapshot.id].result = result
        return result
      } finally {
        busy.value = false
        pending = null
      }
    })()
    return pending
  }
  async function waitForIdle() { await pending?.catch(() => undefined) }
  return { busy, records, run, waitForIdle }
})
