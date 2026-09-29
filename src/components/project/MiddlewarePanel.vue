<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { useProjectStore } from '@/stores/project'
import { backend, toAppError } from '@/api/backend'
import type { MiddlewareTemplate } from '@/types/catalog'
import type { MiddlewareInstance } from '@/types/project'
import { genId } from '@/utils/id'
import InstanceFormDialog from './InstanceFormDialog.vue'

const store = useProjectStore()
const catalog = ref<MiddlewareTemplate[]>([])
const currentServerId = ref('')
const dialogOpen = ref(false)
const editingId = ref<string | null>(null) // null = 新增

onMounted(async () => {
  try {
    catalog.value = (await backend.listCatalog()).templates
  } catch (e) {
    ElMessage.error(`读取中间件目录失败: ${toAppError(e).message}`)
  }
})

const servers = computed(() => store.project?.servers ?? [])
const currentServer = computed(() => servers.value.find((s) => s.id === currentServerId.value))
const instances = computed(
  () => store.project?.instances.filter((i) => i.serverId === currentServerId.value) ?? []
)

// 默认选中第一台服务器
watch(
  servers,
  (list) => {
    if (list.length && !list.some((s) => s.id === currentServerId.value)) {
      currentServerId.value = list[0].id
    }
  },
  { immediate: true }
)

function templateOf(id: string): MiddlewareTemplate | undefined {
  return catalog.value.find((t) => t.id === id)
}

function displayName(inst: MiddlewareInstance) {
  return inst.templateId === 'custom' ? '自定义镜像' : templateOf(inst.templateId)?.displayName ?? inst.templateId
}

function openCreate() {
  if (!currentServerId.value) {
    ElMessage.warning('请先在「服务器清单」中添加服务器')
    return
  }
  editingId.value = null
  dialogOpen.value = true
}

function openEdit(inst: MiddlewareInstance) {
  editingId.value = inst.id
  dialogOpen.value = true
}

async function handleDelete(inst: MiddlewareInstance) {
  try {
    await ElMessageBox.confirm(
      `确定从该服务器移除「${displayName(inst)}（${inst.instanceName}）」？同时会移除相关访问规则。`,
      '删除确认',
      { type: 'warning', confirmButtonText: '删除', cancelButtonText: '取消' }
    )
  } catch {
    return
  }
  store.mutate((p) => {
    p.instances = p.instances.filter((i) => i.id !== inst.id)
    p.networkRules = p.networkRules.filter((r) => r.toPort !== inst.ports[0]?.host || r.toServerId !== inst.serverId)
  })
  ElMessage.success('已删除（保存方案后生效）')
}

/** 保存实例（新增或更新），含 R2 同机宿主端口冲突校验 */
function saveInstance(inst: MiddlewareInstance) {
  const err = validateInstance(inst)
  if (err) {
    ElMessage.warning(err)
    return false
  }
  store.mutate((p) => {
    const idx = p.instances.findIndex((i) => i.id === inst.id)
    if (idx >= 0) {
      p.instances[idx] = inst
    } else {
      p.instances.push(inst)
    }
  })
  dialogOpen.value = false
  ElMessage.success('已保存（方案保存后生效）')
  return true
}

function validateInstance(inst: MiddlewareInstance): string | null {
  if (!inst.instanceName.trim()) return '实例名不能为空'
  if (!/^[a-zA-Z0-9][a-zA-Z0-9_-]*$/.test(inst.instanceName))
    return '实例名仅允许字母数字与 _ -（作为 compose 服务名）'
  if (inst.templateId === 'custom' && !inst.image.trim()) return '镜像引用不能为空'
  for (const port of inst.ports) {
    if (port.host < 1 || port.host > 65535) return `宿主端口 ${port.host} 超出范围 (1-65535)`
  }
  // R2：同机宿主端口冲突（排除自身）
  const others =
    store.project?.instances.filter((i) => i.serverId === inst.serverId && i.id !== inst.id) ?? []
  const ownPorts = inst.ports.map((p) => p.host)
  for (const other of others) {
    for (const op of other.ports) {
      if (ownPorts.includes(op.host)) {
        return `宿主端口 ${op.host} 已被「${other.instanceName}」占用`
      }
    }
  }
  // 实例名同机唯一
  if (others.some((o) => o.instanceName === inst.instanceName)) {
    return `实例名「${inst.instanceName}」在该服务器上已存在`
  }
  return null
}

function emptyInstance(templateId: string): MiddlewareInstance {
  return {
    id: genId('inst'),
    serverId: currentServerId.value,
    templateId,
    image: '',
    digest: '',
    instanceName: '',
    params: {},
    ports: [],
    localImageTar: ''
  }
}
</script>

<template>
  <div>
    <div class="flex justify-between items-center mb-4 gap-4">
      <div class="flex items-center gap-3">
        <span class="text-on-surface-variant text-sm shrink-0">服务器：</span>
        <el-select
          v-model="currentServerId"
          placeholder="选择服务器"
          class="w-64"
          :disabled="servers.length === 0"
        >
          <el-option
            v-for="s in servers"
            :key="s.id"
            :value="s.id"
            :label="`${s.name}（${s.arch}）`"
          />
        </el-select>
      </div>
      <button
        class="px-5 py-1.5 rounded-xl font-headline font-bold text-xs uppercase tracking-wider transition-all bg-gradient-to-br from-primary to-primary-dim text-on-primary hover:opacity-90 active:scale-95"
        @click="openCreate"
      >
        + 添加中间件
      </button>
    </div>

    <el-alert
      v-if="servers.length === 0"
      title="暂无服务器：请先在「服务器清单」页签录入服务器，再编排中间件"
      type="info"
      :closable="false"
      class="mb-4"
    />

    <el-table
      v-else
      :data="instances"
      empty-text="该服务器暂无中间件，点击右上角「添加中间件」"
    >
      <el-table-column label="中间件" min-width="130">
        <template #default="{ row }">
          <span class="font-medium">{{ displayName(row) }}</span>
        </template>
      </el-table-column>
      <el-table-column prop="instanceName" label="实例名" min-width="100">
        <template #default="{ row }">
          <span class="font-mono text-xs">{{ row.instanceName }}</span>
        </template>
      </el-table-column>
      <el-table-column label="镜像" min-width="200" show-overflow-tooltip>
        <template #default="{ row }">
          <span class="font-mono text-xs text-on-surface-variant">{{ row.image || '—' }}</span>
        </template>
      </el-table-column>
      <el-table-column label="端口 (宿主→容器)" min-width="160">
        <template #default="{ row }">
          <div class="flex flex-wrap gap-1">
            <el-tag
              v-for="p in row.ports"
              :key="p.name"
              size="small"
              :type="p.expose ? 'primary' : 'info'"
              class="font-mono"
            >
              {{ p.host }}→{{ p.container }}/{{ p.protocol }}
            </el-tag>
            <span v-if="!row.ports.length" class="text-on-surface-variant/50 text-xs">无</span>
          </div>
        </template>
      </el-table-column>
      <el-table-column label="本地镜像" width="90" align="center">
        <template #default="{ row }">
          <el-tooltip v-if="row.localImageTar" :content="row.localImageTar" placement="top">
            <span class="material-symbols-outlined text-lg text-success">task_alt</span>
          </el-tooltip>
          <el-tooltip v-else content="未指定本地镜像 tar（M0 需手动指定，M1 支持在线拉取）">
            <span class="material-symbols-outled text-lg text-on-surface-variant/30">help</span>
          </el-tooltip>
        </template>
      </el-table-column>
      <el-table-column label="操作" width="120" align="center">
        <template #default="{ row }">
          <el-button link type="primary" size="small" @click="openEdit(row)">编辑</el-button>
          <el-button link type="danger" size="small" @click="handleDelete(row)">删除</el-button>
        </template>
      </el-table-column>
    </el-table>

    <InstanceFormDialog
      v-model="dialogOpen"
      :editing-id="editingId"
      :server="currentServer ?? null"
      :templates="catalog"
      :instances="store.project?.instances ?? []"
      :make-instance="emptyInstance"
      @save="saveInstance"
    />
  </div>
</template>
