<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useProjectStore } from '@/stores/project'
import { toAppError } from '@/api/backend'
import { ElMessage } from 'element-plus'

const router = useRouter()
const store = useProjectStore()
const loading = ref(false)

onMounted(async () => {
  loading.value = true
  try {
    await store.refreshSummaries()
  } catch (e) {
    ElMessage.error(`读取方案列表失败: ${toAppError(e).message}`)
  } finally {
    loading.value = false
  }
})

function fmtTime(iso: string) {
  return iso ? iso.replace('T', ' ').replace(/([+-]\d{2}:\d{2}|Z)$/, '') : '-'
}
</script>

<template>
  <div class="max-w-4xl mx-auto">
    <section class="mb-8">
      <p class="font-mono text-sm text-on-surface-variant flex items-center gap-2 mb-2">
        <span class="w-2.5 h-2.5 bg-success rounded-full pulsing-orb"></span>
        离线交付工作台
      </p>
      <h1 class="font-headline text-3xl font-bold tracking-tight">离线部署运维工具</h1>
      <p class="text-on-surface-variant text-sm mt-2">
        面向政务内网环境：定义服务器与中间件方案 → 跨架构拉取镜像 → 生成每台服务器自包含离线包
      </p>
    </section>

    <div class="grid grid-cols-1 md:grid-cols-2 gap-4 mb-8">
      <button
        class="bg-surface-container-low p-6 rounded-xl border border-outline-variant hover:border-primary transition-colors text-left"
        @click="router.push({ name: 'projects' })"
      >
        <div class="flex items-center gap-3 mb-3">
          <span class="material-symbols-outlined text-primary">add_box</span>
          <h2 class="text-sm font-headline font-bold text-primary uppercase tracking-widest">
            新建 / 管理方案
          </h2>
        </div>
        <p class="text-sm text-on-surface-variant">
          新建部署方案，管理服务器清单、中间件编排与端口矩阵
        </p>
      </button>

      <div class="bg-surface-container-low p-6 rounded-xl border border-outline-variant opacity-60">
        <div class="flex items-center gap-3 mb-3">
          <span class="material-symbols-outlined text-primary">terminal</span>
          <h2 class="text-sm font-headline font-bold text-primary uppercase tracking-widest">
            使用流程
          </h2>
        </div>
        <p class="text-sm text-on-surface-variant font-mono leading-relaxed">
          1. 录入服务器（架构/OS）<br />
          2. 编排中间件与端口<br />
          3. 校验 → 构建离线包 → 摆渡交付
        </p>
      </div>
    </div>

    <!-- 最近方案 -->
    <section>
      <div class="flex items-center justify-between mb-3">
        <h2 class="text-sm font-headline font-bold text-primary uppercase tracking-widest">
          最近方案
        </h2>
        <RouterLink
          :to="{ name: 'projects' }"
          class="text-xs text-on-surface-variant hover:text-primary transition-colors"
          >查看全部 →</RouterLink
        >
      </div>
      <div v-loading="loading" class="flex flex-col gap-2">
        <button
          v-for="p in store.summaries.slice(0, 5)"
          :key="p.id"
          class="bg-surface-container-low px-5 py-3 rounded-xl border border-outline-variant hover:border-primary transition-colors flex items-center gap-4 text-left"
          @click="router.push({ name: 'project-edit', params: { id: p.id } })"
        >
          <span class="material-symbols-outlined text-on-surface-variant">folder_open</span>
          <div class="flex-1 min-w-0">
            <div class="text-sm font-medium truncate">{{ p.name }}</div>
            <div class="text-xs text-on-surface-variant font-mono">
              {{ p.customer || '—' }} · {{ p.serverCount }} 服务器 · {{ p.instanceCount }} 中间件
            </div>
          </div>
          <span class="text-xs text-on-surface-variant/50 font-mono shrink-0">{{
            fmtTime(p.updatedAt)
          }}</span>
        </button>
        <div
          v-if="!loading && store.summaries.length === 0"
          class="text-center text-sm text-on-surface-variant py-8"
        >
          暂无方案，从上方「新建 / 管理方案」开始
        </div>
      </div>
    </section>
  </div>
</template>
