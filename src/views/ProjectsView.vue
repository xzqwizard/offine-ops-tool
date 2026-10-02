<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { open as openFileDialog, save as saveFileDialog } from '@tauri-apps/plugin-dialog'
import { useRouter } from 'vue-router'
import { ElMessage, ElMessageBox } from 'element-plus'
import { useProjectStore } from '@/stores/project'
import { backend, toAppError } from '@/api/backend'
import { ARCH_TEMPLATES, buildProjectFromTemplate, type ArchTemplate } from '@/utils/archTemplates'
import type { MiddlewareTemplate } from '@/types/catalog'

const router = useRouter()
const store = useProjectStore()
const loading = ref(false)
const createOpen = ref(false)
const form = ref({ name: '', customer: '' })

// 复制方案
const cloneOpen = ref(false)
const cloneTarget = ref<{ id: string; name: string } | null>(null)
const cloneName = ref('')

// 从模板创建
const tplOpen = ref(false)
const tplSelected = ref<ArchTemplate | null>(null)
const tplName = ref('')
const tplCustomer = ref('')
const catalogCache: MiddlewareTemplate[] = []

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

async function handleCreate() {
  if (!form.value.name.trim()) {
    ElMessage.warning('请输入方案名称')
    return
  }
  try {
    const p = await store.create(form.value.name, form.value.customer)
    createOpen.value = false
    form.value = { name: '', customer: '' }
    ElMessage.success(`方案「${p.name}」已创建`)
    router.push({ name: 'project-edit', params: { id: p.id } })
  } catch (e) {
    ElMessage.error(`创建失败: ${toAppError(e).message}`)
  }
}

function openClone(id: string, name: string) {
  cloneTarget.value = { id, name }
  cloneName.value = `${name}（副本）`
  cloneOpen.value = true
}

async function handleClone() {
  if (!cloneTarget.value || !cloneName.value.trim()) {
    ElMessage.warning('请输入新方案名称')
    return
  }
  try {
    const p = await backend.cloneProject(cloneTarget.value.id, cloneName.value)
    cloneOpen.value = false
    ElMessage.success(`已复制为「${p.name}」（服务器 IP 已清空待填）`)
    await store.refreshSummaries()
    router.push({ name: 'project-edit', params: { id: p.id } })
  } catch (e) {
    ElMessage.error(`复制失败: ${toAppError(e).message}`)
  }
}

function openTpl() {
  tplSelected.value = null
  tplName.value = ''
  tplCustomer.value = ''
  tplOpen.value = true
}

async function handleTplCreate() {
  if (!tplSelected.value) {
    ElMessage.warning('请选择架构模板')
    return
  }
  if (!tplName.value.trim()) {
    ElMessage.warning('请输入方案名称')
    return
  }
  try {
    let catalog = catalogCache
    if (!catalog.length) {
      catalog = (await backend.listCatalog()).templates
      catalogCache.push(...catalog)
    }
    const p = await store.create(tplName.value, tplCustomer.value)
    // 叠放模板内容后整体保存（create 只落了空方案）
    p.customer = tplCustomer.value
    buildProjectFromTemplate(p, tplSelected.value, catalog)
    const saved = await backend.saveProject(p)
    store.project = saved
    await store.refreshSummaries()
    tplOpen.value = false
    ElMessage.success(`方案「${saved.name}」已按模板生成（补填服务器 IP 后即可校验构建）`)
    router.push({ name: 'project-edit', params: { id: saved.id } })
  } catch (e) {
    ElMessage.error(`模板创建失败: ${toAppError(e).message}`)
  }
}

async function handleDelete(id: string, name: string) {
  try {
    await ElMessageBox.confirm(`确定删除方案「${name}」？该操作不可恢复。`, '删除确认', {
      type: 'warning',
      confirmButtonText: '删除',
      cancelButtonText: '取消'
    })
  } catch {
    return /* 用户取消 */
  }
  try {
    await store.remove(id)
    ElMessage.success('已删除')
  } catch (e) {
    ElMessage.error(`删除失败: ${toAppError(e).message}`)
  }
}

async function handleExport(id: string, name: string) {
  if (!('__TAURI_INTERNALS__' in window)) return
  try {
    const path = await saveFileDialog({
      title: '导出方案',
      defaultPath: `${name}.oppjson`,
      filters: [{ name: '方案文件', extensions: ['oppjson'] }]
    })
    if (!path) return
    const msg = await backend.exportProject(id, path)
    ElMessage.success(msg + '（已排除仓库及中间件密码，导入后需重新填写）')
  } catch (e) {
    ElMessage.error(`导出失败: ${toAppError(e).message}`)
  }
}

async function handleImport() {
  if (!('__TAURI_INTERNALS__' in window)) return
  try {
    const files = await openFileDialog({
      multiple: false,
      title: '导入方案文件',
      filters: [{ name: '方案文件', extensions: ['oppjson'] }]
    })
    const path = Array.isArray(files) ? files[0] : files
    if (!path) return
    const p = await backend.importProject(path)
    ElMessage.success(`已导入「${p.name}」（服务器 IP 沿用原值，可再编辑）`)
    await store.refreshSummaries()
    router.push({ name: 'project-edit', params: { id: p.id } })
  } catch (e) {
    ElMessage.error(`导入失败: ${toAppError(e).message}`)
  }
}

function openProject(id: string) {
  router.push({ name: 'project-edit', params: { id } })
}

function fmtTime(iso: string) {
  if (!iso) return '-'
  return iso.replace('T', ' ').replace(/([+-]\d{2}:\d{2}|Z)$/, '')
}
</script>

<template>
  <div class="max-w-5xl mx-auto">
    <div class="flex justify-between items-center mb-6">
      <h1 class="font-headline text-2xl font-bold tracking-tight">方案管理</h1>
      <button
        class="px-6 py-2 rounded-xl font-headline font-bold text-xs uppercase tracking-wider transition-all bg-gradient-to-br from-primary to-primary-dim text-on-primary hover:opacity-90 active:scale-95"
        @click="createOpen = true"
      >
        + 新建方案
      </button>
      <button
        class="px-5 py-2 rounded-xl text-xs font-bold border border-outline-variant text-on-surface-variant hover:border-primary hover:text-primary transition-colors"
        @click="handleImport"
      >
        导入方案
      </button>
    </div>

    <div class="bg-surface-container-low rounded-xl border border-outline-variant">
      <el-table
        :data="store.summaries"
        v-loading="loading"
        empty-text="暂无方案，点击右上角「新建方案」开始"
        style="width: 100%"
      >
        <el-table-column prop="name" label="方案名称" min-width="160">
          <template #default="{ row }">
            <button class="text-primary hover:underline font-medium" @click="openProject(row.id)">
              {{ row.name }}
            </button>
          </template>
        </el-table-column>
        <el-table-column prop="customer" label="客户/项目" min-width="120">
          <template #default="{ row }">
            <span class="text-on-surface-variant">{{ row.customer || '—' }}</span>
          </template>
        </el-table-column>
        <el-table-column label="服务器" width="80" align="center">
          <template #default="{ row }">
            <el-tag size="small" type="info">{{ row.serverCount }}</el-tag>
          </template>
        </el-table-column>
        <el-table-column label="中间件" width="80" align="center">
          <template #default="{ row }">
            <el-tag size="small" type="info">{{ row.instanceCount }}</el-tag>
          </template>
        </el-table-column>
        <el-table-column label="更新时间" width="160">
          <template #default="{ row }">
            <span class="font-mono text-xs text-on-surface-variant">{{ fmtTime(row.updatedAt) }}</span>
          </template>
        </el-table-column>
        <el-table-column label="操作" width="210" align="center">
          <template #default="{ row }">
            <el-button link type="primary" size="small" @click="openProject(row.id)">打开</el-button>
            <el-button link type="primary" size="small" @click="openClone(row.id, row.name)">复制</el-button>
            <el-button link type="primary" size="small" @click="handleExport(row.id, row.name)">导出</el-button>
            <el-button link type="danger" size="small" @click="handleDelete(row.id, row.name)">
              删除
            </el-button>
          </template>
        </el-table-column>
      </el-table>
    </div>

    <el-dialog v-model="createOpen" title="新建部署方案" width="460">
      <el-form label-width="90px" @submit.prevent>
        <el-form-item label="方案名称" required>
          <el-input v-model="form.name" placeholder="如：XX 政务平台一期" maxlength="60" />
        </el-form-item>
        <el-form-item label="客户/项目">
          <el-input v-model="form.customer" placeholder="如：XX 厅（选填）" maxlength="60" />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="tplOpen = true">从模板创建…</el-button>
        <el-button @click="createOpen = false">取消</el-button>
        <el-button type="primary" @click="handleCreate">创建</el-button>
      </template>
    </el-dialog>

    <!-- 复制方案 -->
    <el-dialog v-model="cloneOpen" title="复制方案" width="460">
      <p class="text-sm text-on-surface-variant mb-4">
        复制「{{ cloneTarget?.name }}」的全部服务器、中间件与端口规则；服务器 IP 清空待填。
      </p>
      <el-input v-model="cloneName" placeholder="新方案名称" maxlength="60" />
      <template #footer>
        <el-button @click="cloneOpen = false">取消</el-button>
        <el-button type="primary" @click="handleClone">复制并打开</el-button>
      </template>
    </el-dialog>

    <!-- 从架构模板创建 -->
    <el-dialog v-model="tplOpen" title="从架构模板创建方案" width="560">
      <div class="flex flex-col gap-2 mb-4">
        <button
          v-for="t in ARCH_TEMPLATES"
          :key="t.id"
          class="text-left px-4 py-3 rounded-lg border transition-colors"
          :class="tplSelected?.id === t.id ? 'border-primary bg-surface-container-high' : 'border-outline-variant bg-surface-container-low hover:border-primary/50'"
          @click="tplSelected = t"
        >
          <div class="text-sm font-medium">{{ t.name }}</div>
          <div class="text-xs text-on-surface-variant mt-0.5">{{ t.desc }}</div>
        </button>
      </div>
      <div class="grid grid-cols-2 gap-3">
        <el-input v-model="tplName" placeholder="方案名称（必填）" maxlength="60" />
        <el-input v-model="tplCustomer" placeholder="客户/项目（选填）" maxlength="60" />
      </div>
      <div class="text-[10px] text-on-surface-variant/50 mt-2">
        将自动生成服务器与中间件实例骨架（密码自动生成，IP 留空待填），打开后核对参数即可校验构建
      </div>
      <template #footer>
        <el-button @click="tplOpen = false">取消</el-button>
        <el-button type="primary" :disabled="!tplSelected" @click="handleTplCreate">生成并打开</el-button>
      </template>
    </el-dialog>
  </div>
</template>
