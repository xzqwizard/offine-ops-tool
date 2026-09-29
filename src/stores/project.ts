import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { backend } from '@/api/backend'
import type { Project, ProjectSummary } from '@/types/project'

let saveTimer: number | undefined

export const useProjectStore = defineStore('project', () => {
  const project = ref<Project | null>(null)
  const summaries = ref<ProjectSummary[]>([])
  const dirty = ref(false)
  const saving = ref(false)

  const serverCount = computed(() => project.value?.servers.length ?? 0)
  const instanceCount = computed(() => project.value?.instances.length ?? 0)

  async function refreshSummaries() {
    summaries.value = await backend.listProjects()
  }

  async function open(id: string) {
    project.value = await backend.loadProject(id)
    dirty.value = false
  }

  async function create(name: string, customer: string): Promise<Project> {
    project.value = await backend.createProject(name, customer)
    dirty.value = false
    await refreshSummaries()
    return project.value
  }

  async function save() {
    if (!project.value) return
    saving.value = true
    try {
      project.value = await backend.saveProject(project.value)
      dirty.value = false
      await refreshSummaries()
    } finally {
      saving.value = false
    }
  }

  /** 修改并立即持久化（表单新增/修改/删除后自动保存） */
  async function commit(fn: (p: Project) => void) {
    if (!project.value) return
    fn(project.value)
    await save()
  }

  /** 修改并防抖自动保存（用于名称等连续输入场景） */
  function scheduleSave(fn: (p: Project) => void, delay = 600) {
    if (!project.value) return
    fn(project.value)
    dirty.value = true
    if (saveTimer) window.clearTimeout(saveTimer)
    saveTimer = window.setTimeout(() => {
      save().catch(() => {
        /* 失败时保持 dirty，用户可见"待保存"提示 */
      })
    }, delay)
  }

  async function remove(id: string) {
    await backend.deleteProject(id)
    if (project.value?.id === id) {
      project.value = null
      dirty.value = false
    }
    await refreshSummaries()
  }

  function close() {
    project.value = null
    dirty.value = false
  }

  return {
    project,
    summaries,
    dirty,
    saving,
    serverCount,
    instanceCount,
    refreshSummaries,
    open,
    create,
    save,
    commit,
    scheduleSave,
    remove,
    close
  }
})
