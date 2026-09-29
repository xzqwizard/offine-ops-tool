<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { ElMessage, ElMessageBox } from 'element-plus'
import { useProjectStore } from '@/stores/project'
import { toAppError } from '@/api/backend'

const router = useRouter()
const store = useProjectStore()
const loading = ref(false)
const createOpen = ref(false)
const form = ref({ name: '', customer: '' })

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
        <el-table-column label="操作" width="120" align="center">
          <template #default="{ row }">
            <el-button link type="primary" size="small" @click="openProject(row.id)">打开</el-button>
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
        <el-button @click="createOpen = false">取消</el-button>
        <el-button type="primary" @click="handleCreate">创建</el-button>
      </template>
    </el-dialog>
  </div>
</template>
