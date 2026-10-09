import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { backend } from '@/api/backend'
import type { Project, ProjectSummary } from '@/types/project'
import { mergeSavedDraft } from '@/utils/projectDraft'

export const useProjectStore = defineStore('project', () => {
  let saveTimer: number | undefined
  let saveChain: Promise<void> = Promise.resolve()
  let loadSeq = 0
  const loading = ref(false)
  const project = ref<Project | null>(null)
  const summaries = ref<ProjectSummary[]>([])
  const dirty = ref(false)
  const saving = ref(false)
  /** 方案内容修订号：任何变更（无论保存方式）自增，供校验结果等做过期判断 */
  const revision = ref(0)

  const serverCount = computed(() => project.value?.servers.length ?? 0)
  const instanceCount = computed(() => project.value?.instances.length ?? 0)

  function touch() {
    revision.value++
  }

  async function refreshSummaries() {
    summaries.value = await backend.listProjects()
  }

  /** 执行保存（排队串行）。仅"发起后无编辑"的响应才回写内存，防止在途快照回滚用户输入 */
  function save(): Promise<void> {
    const run = async () => {
      const snapshot = project.value ? JSON.parse(JSON.stringify(project.value)) as Project : null
      const revBefore = revision.value
      if (!snapshot) return
      saving.value = true
      try {
        const saved = await backend.saveProject(snapshot)
        if (!project.value || saved.id !== project.value.id) return
        // 响应回来前发生了编辑：不回写（会丢编辑），保持 dirty 由后续保存落盘
        if (revision.value !== revBefore) return
        project.value = saved
        dirty.value = false
        await refreshSummaries().catch(() => undefined)
      } finally {
        saving.value = false
      }
    }
    const result = saveChain.then(run)
    saveChain = result.catch(() => undefined)
    return result
  }

  /** 修改并立即持久化（表单新增/修改/删除后自动保存） */
  async function commit(fn: (p: Project) => void) {
    const run = async () => {
      if (!project.value) return
      const before = JSON.parse(JSON.stringify(project.value)) as Project
      const draft = JSON.parse(JSON.stringify(before)) as Project
      const revBefore = revision.value
      fn(draft)
      saving.value = true
      try {
        const saved = await backend.saveProject(draft)
        if (project.value?.id !== saved.id) return
        const edited = revision.value !== revBefore
        project.value = edited ? mergeSavedDraft(before, saved, project.value) : saved
        touch()
        dirty.value = edited
        await refreshSummaries().catch(() => undefined)
      } finally { saving.value = false }
    }
    const result = saveChain.then(run)
    saveChain = result.catch(() => undefined)
    await result
  }

  /** 修改并防抖自动保存（名称等连续输入场景） */
  function scheduleSave(fn: (p: Project) => void, delay = 600) {
    if (!project.value) return
    fn(project.value)
    touch()
    dirty.value = true
    if (saveTimer) window.clearTimeout(saveTimer)
    saveTimer = window.setTimeout(() => {
      saveTimer = undefined
      save().catch(() => {
        /* 失败保持 dirty，界面显示"待保存" */
      })
    }, delay)
  }

  /** 立即落盘未保存的修改（切方案/删除前调用）：flush 防抖定时器并等待在途/排队保存完成 */
  async function flushPending() {
    if (saveTimer) {
      window.clearTimeout(saveTimer)
      saveTimer = undefined
    }
    if (dirty.value) await save()
    await saveChain
    if (dirty.value) await save()
  }

  async function open(id: string) {
    const seq = ++loadSeq
    await flushPending()
    if (seq !== loadSeq) return
    project.value = null
    loading.value = true
    try {
      const loaded = await backend.loadProject(id)
      if (seq !== loadSeq) return
      project.value = loaded
      dirty.value = false
      revision.value++
    } finally {
      if (seq === loadSeq) loading.value = false
    }
  }

  async function create(name: string, customer: string): Promise<Project> {
    await flushPending()
    project.value = await backend.createProject(name, customer)
    dirty.value = false
    await refreshSummaries()
    return project.value
  }

  async function remove(id: string) {
    await flushPending()
    await backend.deleteProject(id)
    if (project.value?.id === id) {
      project.value = null
      dirty.value = false
    }
    await refreshSummaries()
  }

  async function close() {
    await flushPending()
    ++loadSeq
    project.value = null
    dirty.value = false
  }

  return {
    project,
    loading,
    summaries,
    dirty,
    saving,
    revision,
    serverCount,
    instanceCount,
    refreshSummaries,
    open,
    create,
    save,
    commit,
    scheduleSave,
    flushPending,
    remove,
    close
  }
})
