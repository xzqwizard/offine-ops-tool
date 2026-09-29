import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { backend } from '@/api/backend'
import type { Project, ProjectSummary } from '@/types/project'

let saveTimer: number | undefined
/** 保存序号：丢弃乱序返回的过期保存响应，防止旧快照覆盖新数据 */
let saveSeq = 0

export const useProjectStore = defineStore('project', () => {
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

  /** 执行保存；仅最新一次保存的结果会写回内存 */
  async function save() {
    if (!project.value) return
    const seq = ++saveSeq
    saving.value = true
    try {
      const saved = await backend.saveProject(project.value)
      // 过期响应丢弃（更晚发起的保存已在途/已完成）
      if (seq !== saveSeq || !project.value || saved.id !== project.value.id) return
      project.value = saved
      if (seq === saveSeq) dirty.value = false
      await refreshSummaries()
    } finally {
      if (seq === saveSeq) saving.value = false
    }
  }

  /** 修改并立即持久化（表单新增/修改/删除后自动保存） */
  async function commit(fn: (p: Project) => void) {
    if (!project.value) return
    fn(project.value)
    touch()
    dirty.value = true // 先置位：保存失败时界面保持"待保存"警示
    await save()
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

  /** 立即落盘未保存的防抖修改（切方案/删除前调用） */
  async function flushPending() {
    if (saveTimer) {
      window.clearTimeout(saveTimer)
      saveTimer = undefined
      if (dirty.value) await save()
    }
  }

  async function open(id: string) {
    await flushPending()
    project.value = await backend.loadProject(id)
    dirty.value = false
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

  function close() {
    project.value = null
    dirty.value = false
  }

  return {
    project,
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
