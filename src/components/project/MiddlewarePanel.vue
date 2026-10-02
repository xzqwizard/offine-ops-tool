<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { useProjectStore } from '@/stores/project'
import { backend, toAppError } from '@/api/backend'
import type { MiddlewareTemplate } from '@/types/catalog'
import type { MiddlewareInstance } from '@/types/project'
import InstanceFormDialog from './InstanceFormDialog.vue'

const store = useProjectStore()
const availableCatalog = ref<MiddlewareTemplate[]>([])
const catalog = computed(() => {
  const snapshots = store.project?.templateSnapshots ?? []
  return [...availableCatalog.value.filter(t => !snapshots.some(s => s.id === t.id)), ...snapshots]
})
const dialogOpen = ref(false)
const editingId = ref<string | null>(null) // null = 新增

async function refreshCatalog() {
  try {
    availableCatalog.value = (await backend.listCatalog()).templates
  } catch (e) {
    ElMessage.error(`读取中间件目录失败: ${toAppError(e).message}`)
  }
}
onMounted(() => { refreshCatalog(); window.addEventListener('catalog-updated', refreshCatalog) })
onUnmounted(() => window.removeEventListener('catalog-updated', refreshCatalog))
watch(dialogOpen, open => { if (open) refreshCatalog() })

const servers = computed(() => store.project?.servers ?? [])
const instances = computed(() => store.project?.instances ?? [])

function serverName(id: string): string {
  const s = servers.value.find((s) => s.id === id)
  return s ? `${s.name}（${s.arch}）` : id
}

function templateOf(id: string): MiddlewareTemplate | undefined {
  return catalog.value.find((t) => t.id === id)
}

function displayName(inst: MiddlewareInstance) {
  return inst.templateId === 'custom'
    ? '自定义镜像'
    : templateOf(inst.templateId)?.displayName ?? inst.templateId
}

function openCreate() {
  if (servers.value.length === 0) {
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
      `确定移除「${serverName(inst.serverId)}」上的「${displayName(inst)}（${inst.instanceName}）」？相关访问规则将一并移除。`,
      '删除确认',
      { type: 'warning', confirmButtonText: '删除', cancelButtonText: '取消' }
    )
  } catch {
    return
  }
  try {
    await store.commit((p) => {
      p.instances = p.instances.filter((i) => i.id !== inst.id)
      p.networkRules = p.networkRules.filter(
        (r) =>
          !(
            r.toServerId === inst.serverId &&
            inst.ports.some((p) => p.host === r.toPort && p.protocol === r.protocol && p.expose)
          )
      )
    })
    ElMessage.success('已删除并保存')
  } catch (e) {
    ElMessage.error(`删除失败: ${toAppError(e).message}`)
  }
}

/** 批量保存实例（多选服务器部署）：逐个校验，全部通过才落盘 */
function saveInstances(insts: MiddlewareInstance[]) {
  for (const inst of insts) {
    const err = validateInstance(inst)
    if (err) {
      ElMessage.warning(err)
      return
    }
  }
  store
    .commit((p) => {
      for (const inst of insts) {
        p.instances.push(inst)
      }
    })
    .then(() => {
      dialogOpen.value = false
      ElMessage.success(`已在 ${insts.length} 台服务器上添加`)
    })
    .catch((e) => ElMessage.error(`保存失败: ${toAppError(e).message}`))
}

/** 保存实例（新增或更新），含 R2 同机宿主端口冲突校验；通过后自动保存 */
function saveInstance(inst: MiddlewareInstance) {
  const err = validateInstance(inst)
  if (err) {
    ElMessage.warning(err)
    return
  }
  store
    .commit((p) => {
      const idx = p.instances.findIndex((i) => i.id === inst.id)
      if (idx >= 0) {
        p.instances[idx] = inst
      } else {
        p.instances.push(inst)
      }
    })
    .then(() => {
      dialogOpen.value = false
      ElMessage.success('已保存')
    })
    .catch((e) => ElMessage.error(`保存失败: ${toAppError(e).message}`))
}

function validateInstance(inst: MiddlewareInstance): string | null {
  if (!inst.serverId) return '请选择目标服务器'
  if (!store.project?.servers.some((s) => s.id === inst.serverId)) return '目标服务器不存在'
  if (!inst.instanceName.trim()) return '实例名不能为空'
  if (!/^[a-zA-Z0-9][a-zA-Z0-9_-]*$/.test(inst.instanceName))
    return '实例名仅允许字母数字与 _ -（作为 compose 服务名）'
  if (!inst.image.trim()) return '镜像引用不能为空'
  for (const port of inst.ports) {
    if (port.expose && (port.host < 1 || port.host > 65535)) return `宿主端口 ${port.host} 超出范围 (1-65535)`
  }
  // R2：同机宿主端口冲突（排除自身）
  const others =
    store.project?.instances.filter((i) => i.serverId === inst.serverId && i.id !== inst.id) ?? []
  const ownPorts = inst.ports.filter(p => p.expose).map(p => `${p.host}/${p.protocol}`)
  for (const other of others) {
    for (const op of other.ports) {
      if (op.expose && ownPorts.includes(`${op.host}/${op.protocol}`)) {
        return `服务器「${serverName(inst.serverId)}」端口 ${op.host} 已被「${other.instanceName}」占用`
      }
    }
  }
  // 实例名同机唯一
  if (others.some((o) => o.instanceName === inst.instanceName)) {
    return `实例名「${inst.instanceName}」在该服务器上已存在`
  }
  return null
}
</script>

<template>
  <div>
    <div class="flex justify-between items-center mb-4">
      <p class="text-on-surface-variant text-sm">
        共 <span class="text-primary font-mono">{{ instances.length }}</span> 个实例
        <span class="text-xs text-on-surface-variant/60 ml-2">新增/修改/删除后自动保存</span>
      </p>
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

    <el-table v-else :data="instances" empty-text="暂无中间件，点击右上角「添加中间件」">
      <el-table-column label="服务器" min-width="150">
        <template #default="{ row }">
          <span class="text-sm">{{ serverName(row.serverId) }}</span>
        </template>
      </el-table-column>
      <el-table-column label="中间件" min-width="120">
        <template #default="{ row }">
          <span class="font-medium">{{ displayName(row) }}</span>
        </template>
      </el-table-column>
      <el-table-column prop="instanceName" label="实例名" min-width="90">
        <template #default="{ row }">
          <span class="font-mono text-xs">{{ row.instanceName }}</span>
        </template>
      </el-table-column>
      <el-table-column label="镜像" min-width="180" show-overflow-tooltip>
        <template #default="{ row }">
          <span class="font-mono text-xs text-on-surface-variant">{{ row.image || '—' }}</span>
        </template>
      </el-table-column>
      <el-table-column label="端口 (宿主→容器)" min-width="150">
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
      <el-table-column label="本地镜像" width="80" align="center">
        <template #default="{ row }">
          <el-tooltip v-if="row.localImageTar" :content="row.localImageTar" placement="top">
            <span class="material-symbols-outlined text-lg text-success">task_alt</span>
          </el-tooltip>
          <el-tooltip v-else content="未指定本地镜像 tar（M0 需手动指定，M1 支持在线拉取）">
            <span class="material-symbols-outlined text-lg text-on-surface-variant/30">help</span>
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
      :servers="servers"
      :templates="catalog"
      @save="saveInstance"
      @save-many="saveInstances"
    />
  </div>
</template>
