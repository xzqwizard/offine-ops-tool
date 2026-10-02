<script setup lang="ts">
import { formatTime } from '@/utils/time'
import { ref, computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { ElMessage } from 'element-plus'
import { useProjectStore } from '@/stores/project'
import { backend, toAppError } from '@/api/backend'
import type { BuildHistoryEntry } from '@/types/buildHistory'

const router = useRouter()
const store = useProjectStore()
const loading = ref(true)
const history = ref<BuildHistoryEntry[]>([])
const disk = ref<{ freeBytes: number; totalBytes: number } | null>(null)

onMounted(async () => {
  loading.value = true
  try {
    await store.refreshSummaries()
    const [h, storage] = await Promise.all([
      backend.listBuildHistory(),
      backend.getStorageInfo().catch(() => null)
    ])
    history.value = h
    if (storage) {
      disk.value = await backend.getDiskSpace(storage.artifactRoot).catch(() => null)
    }
  } catch (e) {
    ElMessage.error(`读取数据失败: ${toAppError(e).message}`)
  } finally {
    loading.value = false
  }
})

/** projectId -> 最近构建 + 次数 */
const buildStats = computed(() => {
  const m = new Map<string, { last: string; count: number; size: number }>()
  for (const h of history.value) {
    if (h.status !== 'complete') continue
    const cur = m.get(h.projectId) ?? { last: '', count: 0, size: 0 }
    cur.count += 1
    cur.size += h.totalSizeBytes
    if (h.generatedAt > cur.last) cur.last = h.generatedAt
    m.set(h.projectId, cur)
  }
  return m
})

function fmtSize(bytes: number): string {
  if (bytes >= 1073741824) return `${(bytes / 1073741824).toFixed(1)} GB`
  if (bytes >= 1048576) return `${(bytes / 1048576).toFixed(0)} MB`
  return `${(bytes / 1024).toFixed(0)} KB`
}

const fmtTime = formatTime
</script>

<template>
  <div class="max-w-5xl mx-auto">
    <section class="mb-6">
      <p class="font-mono text-sm text-on-surface-variant flex items-center gap-2 mb-2">
        <span class="w-2.5 h-2.5 bg-success rounded-full pulsing-orb"></span>
        离线交付工作台
      </p>
      <h1 class="font-headline text-3xl font-bold tracking-tight">离线部署运维工具</h1>
      <p class="text-on-surface-variant text-sm mt-2">
        面向政务内网环境：定义服务器与中间件方案 → 跨架构拉取镜像 → 生成每台服务器自包含离线包
      </p>
    </section>

    <!-- 磁盘概览 -->
    <div v-if="disk" class="grid grid-cols-3 gap-3 mb-8">
      <div class="bg-surface-container-low rounded-xl border border-outline-variant p-4 text-center">
        <div class="text-xl font-headline font-bold text-primary">{{ store.summaries.length }}</div>
        <div class="text-xs text-on-surface-variant mt-1">方案</div>
      </div>
      <div class="bg-surface-container-low rounded-xl border border-outline-variant p-4 text-center">
        <div class="text-xl font-headline font-bold text-primary">{{ history.length }}</div>
        <div class="text-xs text-on-surface-variant mt-1">累计构建</div>
      </div>
      <div class="bg-surface-container-low rounded-xl border border-outline-variant p-4 text-center">
        <div
          class="text-xl font-headline font-bold"
          :class="disk.freeBytes < 10 * 1073741824 ? 'text-error' : 'text-success'"
        >
          {{ fmtSize(disk.freeBytes) }}
        </div>
        <div class="text-xs text-on-surface-variant mt-1">产物盘剩余 / {{ fmtSize(disk.totalBytes) }}</div>
      </div>
    </div>

    <!-- 项目卡片 -->
    <section>
      <div class="flex items-center justify-between mb-3">
        <h2 class="text-sm font-headline font-bold text-primary uppercase tracking-widest">我的方案</h2>
        <RouterLink :to="{ name: 'projects' }" class="text-xs text-on-surface-variant hover:text-primary">
          管理 / 新建 →
        </RouterLink>
      </div>
      <div v-loading="loading" class="grid grid-cols-1 md:grid-cols-2 gap-3">
        <button
          v-for="p in store.summaries"
          :key="p.id"
          class="bg-surface-container-low px-5 py-4 rounded-xl border border-outline-variant hover:border-primary transition-colors text-left"
          @click="router.push({ name: 'project-edit', params: { id: p.id } })"
        >
          <div class="flex items-center gap-3">
            <span class="material-symbols-outlined text-on-surface-variant">dns</span>
            <span class="font-medium flex-1 truncate">{{ p.name }}</span>
            <span
              v-if="buildStats.get(p.id)"
              class="font-mono text-[10px] px-1.5 py-0.5 rounded bg-surface-container-highest"
              >{{ buildStats.get(p.id)!.count }} 次构建</span
            >
          </div>
          <div class="text-xs text-on-surface-variant mt-2 flex justify-between">
            <span>{{ p.serverCount }} 服务器 · {{ p.instanceCount }} 中间件</span>
            <span class="font-mono">{{
              buildStats.get(p.id) ? fmtTime(buildStats.get(p.id)!.last) : '未构建'
            }}</span>
          </div>
        </button>
        <div v-if="!loading && !store.summaries.length" class="text-center text-sm text-on-surface-variant py-8">
          暂无方案，从「管理 / 新建」开始（支持架构模板一键创建）
        </div>
      </div>
    </section>
  </div>
</template>
