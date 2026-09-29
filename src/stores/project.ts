import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { backend } from '@/api/backend'
import type { Project, ProjectSummary } from '@/types/project'

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

  async function remove(id: string) {
    await backend.deleteProject(id)
    if (project.value?.id === id) {
      project.value = null
      dirty.value = false
    }
    await refreshSummaries()
  }

  /** 修改方案内容统一走此入口，确保 dirty 标记不遗漏 */
  function mutate(fn: (p: Project) => void) {
    if (!project.value) return
    fn(project.value)
    dirty.value = true
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
    remove,
    mutate,
    close
  }
})
