<script setup lang="ts">
import { ref, computed, watch, onMounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { ElMessage } from 'element-plus'
import { useProjectStore } from '@/stores/project'
import { toAppError } from '@/api/backend'
import ServersPanel from '@/components/project/ServersPanel.vue'
import MiddlewarePanel from '@/components/project/MiddlewarePanel.vue'
import NetworkPanel from '@/components/project/NetworkPanel.vue'
import ValidatePanel from '@/components/project/ValidatePanel.vue'
import BuildPanel from '@/components/project/BuildPanel.vue'

const route = useRoute()
const router = useRouter()
const store = useProjectStore()
const activeTab = ref('servers')
const loading = ref(false)

onMounted(async () => {
  const id = route.params.id as string
  loading.value = true
  try {
    await store.open(id)
  } catch (e) {
    ElMessage.error(`打开方案失败: ${toAppError(e).message}`)
    router.replace({ name: 'projects' })
  } finally {
    loading.value = false
  }
})

// 同一组件实例下切换不同方案时重新加载
watch(
  () => route.params.id,
  async (id) => {
    if (id && route.name === 'project-edit') {
      try {
        await store.open(id as string)
      } catch (e) {
        ElMessage.error(`打开方案失败: ${toAppError(e).message}`)
      }
    }
  }
)

// 名称/客户连续输入 → 防抖自动保存
const nameModel = computed({
  get: () => store.project?.name ?? '',
  set: (v: string) => store.scheduleSave((p) => (p.name = v))
})
const customerModel = computed({
  get: () => store.project?.customer ?? '',
  set: (v: string) => store.scheduleSave((p) => (p.customer = v))
})
</script>

<template>
  <div class="max-w-6xl mx-auto" v-loading="loading">
    <!-- 方案头部：名称/客户 自动保存 -->
    <div class="flex items-center gap-4 mb-6">
      <div class="flex-1 min-w-0">
        <div class="flex items-center gap-3">
          <span
            class="font-mono text-xs px-2 py-0.5 rounded bg-surface-container-high text-on-surface-variant"
            >方案</span
          >
          <input
            v-model="nameModel"
            class="bg-transparent font-headline text-2xl font-bold tracking-tight outline-none border-b border-transparent focus:border-primary min-w-0 flex-1"
          />
        </div>
        <input
          v-model="customerModel"
          placeholder="客户/项目（选填）"
          class="bg-transparent text-sm text-on-surface-variant outline-none border-b border-transparent focus:border-primary mt-1"
        />
      </div>
      <div class="flex items-center gap-2 font-mono text-xs">
        <template v-if="store.saving">
          <span class="w-2 h-2 rounded-full bg-warning pulsing-orb"></span>
          <span class="text-warning">保存中…</span>
        </template>
        <template v-else-if="store.dirty">
          <span class="w-2 h-2 rounded-full bg-warning"></span>
          <span class="text-on-surface-variant">待保存</span>
        </template>
        <template v-else>
          <span class="w-2 h-2 rounded-full bg-success"></span>
          <span class="text-on-surface-variant/50">已自动保存</span>
        </template>
      </div>
    </div>

    <!-- 编辑区：分页签 -->
    <div class="bg-surface-container-low rounded-xl border border-outline-variant p-5">
      <el-tabs v-model="activeTab">
        <el-tab-pane label="服务器清单" name="servers">
          <ServersPanel />
        </el-tab-pane>
        <el-tab-pane label="中间件编排" name="middleware">
          <MiddlewarePanel />
        </el-tab-pane>
        <el-tab-pane label="端口矩阵" name="network">
          <NetworkPanel />
        </el-tab-pane>
        <el-tab-pane label="校验" name="validate">
          <ValidatePanel />
        </el-tab-pane>
        <el-tab-pane label="构建" name="build">
          <BuildPanel />
        </el-tab-pane>
      </el-tabs>
    </div>
  </div>
</template>
