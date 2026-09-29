<script setup lang="ts">
import { ref, computed } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { useProjectStore } from '@/stores/project'
import { genId } from '@/utils/id'
import { exposedPortsOf } from '@/utils/validate'
import type { NetworkRule } from '@/types/project'

const store = useProjectStore()
const dialogOpen = ref(false)
const editingRuleId = ref<string | null>(null)
const form = ref<NetworkRule>(emptyRule())

function emptyRule(): NetworkRule {
  return {
    id: '',
    fromServerId: '',
    toServerId: '',
    toPort: 0,
    protocol: 'tcp',
    description: ''
  }
}

const servers = computed(() => store.project?.servers ?? [])
const rules = computed(() => store.project?.networkRules ?? [])

/** 矩阵行：每个暴露端口一行 */
const matrixRows = computed(() => {
  const rows: { toServerId: string; toServerName: string; host: number; protocol: string; instanceName: string }[] = []
  for (const s of servers.value) {
    for (const p of exposedPortsOf(store.project!, s.id)) {
      rows.push({
        toServerId: s.id,
        toServerName: s.name,
        host: p.host,
        protocol: p.protocol,
        instanceName: p.instance.instanceName
      })
    }
  }
  return rows
})

function hasRule(fromId: string, toServerId: string, port: number): boolean {
  return rules.value.some(
    (r) => r.fromServerId === fromId && r.toServerId === toServerId && r.toPort === port
  )
}

/** 点击矩阵单元格切换放行 */
function toggleCell(fromId: string, row: { toServerId: string; host: number }) {
  const existing = rules.value.find(
    (r) => r.fromServerId === fromId && r.toServerId === row.toServerId && r.toPort === row.host
  )
  if (existing) {
    store.mutate((p) => {
      p.networkRules = p.networkRules.filter((r) => r.id !== existing.id)
    })
  } else {
    store.mutate((p) => {
      p.networkRules.push({
        id: genId('rule'),
        fromServerId: fromId,
        toServerId: row.toServerId,
        toPort: row.host,
        protocol: 'tcp',
        description: ''
      })
    })
  }
}

function targetPortOptions(toServerId: string) {
  return exposedPortsOf(store.project!, toServerId).map((p) => ({
    value: p.host,
    label: `${p.host}（${p.instance.instanceName}）`
  }))
}

function openCreate() {
  if (servers.value.length === 0) {
    ElMessage.warning('请先录入服务器')
    return
  }
  editingRuleId.value = null
  const rule = emptyRule()
  rule.fromServerId = servers.value[0].id
  rule.toServerId = servers.value[0].id
  form.value = rule
  dialogOpen.value = true
}

function openEdit(rule: NetworkRule) {
  editingRuleId.value = rule.id
  form.value = JSON.parse(JSON.stringify(rule))
  dialogOpen.value = true
}

function handleSave() {
  const f = form.value
  if (!f.fromServerId || !f.toServerId) {
    ElMessage.warning('请选择来源与目标服务器')
    return
  }
  if (!f.toPort) {
    ElMessage.warning('请选择目标端口')
    return
  }
  const dup = rules.value.some(
    (r) =>
      r.id !== editingRuleId.value &&
      r.fromServerId === f.fromServerId &&
      r.toServerId === f.toServerId &&
      r.toPort === f.toPort
  )
  if (dup) {
    ElMessage.warning('该访问规则已存在')
    return
  }
  store.mutate((p) => {
    if (editingRuleId.value) {
      const idx = p.networkRules.findIndex((r) => r.id === editingRuleId.value)
      if (idx >= 0) p.networkRules[idx] = f
    } else {
      f.id = genId('rule')
      p.networkRules.push(f)
    }
  })
  dialogOpen.value = false
  ElMessage.success('已保存（保存方案后生效）')
}

async function handleDelete(rule: NetworkRule) {
  try {
    await ElMessageBox.confirm('确定删除该访问规则？', '删除确认', {
      type: 'warning',
      confirmButtonText: '删除',
      cancelButtonText: '取消'
    })
  } catch {
    return
  }
  store.mutate((p) => {
    p.networkRules = p.networkRules.filter((r) => r.id !== rule.id)
  })
}

function serverName(id: string): string {
  return servers.value.find((s) => s.id === id)?.name ?? id
}
</script>

<template>
  <div class="flex flex-col gap-6">
    <!-- 端口访问矩阵 -->
    <section>
      <div class="flex justify-between items-center mb-3">
        <h3 class="text-sm font-headline font-bold text-primary uppercase tracking-widest">
          端口访问矩阵
        </h3>
        <span class="text-xs text-on-surface-variant">点击单元格切换放行（列=来源，行=目标端口）</span>
      </div>
      <div class="bg-surface-container-low rounded-xl border border-outline-variant p-4 overflow-x-auto">
        <table v-if="servers.length && matrixRows.length" class="w-full text-sm">
          <thead>
            <tr>
              <th class="text-left text-on-surface-variant font-medium px-3 py-2 sticky left-0 bg-surface-container-low">
                目标端口
              </th>
              <th
                v-for="s in servers"
                :key="s.id"
                class="px-3 py-2 text-on-surface-variant font-medium whitespace-nowrap"
              >
                {{ s.name }}
              </th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="row in matrixRows"
              :key="`${row.toServerId}:${row.host}`"
              class="border-t border-outline-variant"
            >
              <td class="px-3 py-1.5 font-mono text-xs whitespace-nowrap sticky left-0 bg-surface-container-low">
                {{ row.toServerName }}:<span class="text-primary">{{ row.host }}</span>
                <span class="text-on-surface-variant/50"> / {{ row.instanceName }}</span>
              </td>
              <td v-for="s in servers" :key="s.id" class="text-center">
                <button
                  v-if="s.id !== row.toServerId"
                  class="w-8 h-8 rounded-lg inline-flex items-center justify-center transition-all"
                  :class="
                    hasRule(s.id, row.toServerId, row.host)
                      ? 'bg-success/20 text-success hover:bg-success/30'
                      : 'bg-surface-container text-on-surface-variant/20 hover:bg-surface-container-high'
                  "
                  :title="hasRule(s.id, row.toServerId, row.host) ? '点击取消放行' : '点击放行'"
                  @click="toggleCell(s.id, row)"
                >
                  <span class="material-symbols-outlined text-base">{{
                    hasRule(s.id, row.toServerId, row.host) ? 'check' : 'add'
                  }}</span>
                </button>
                <span v-else class="text-on-surface-variant/30 text-xs">本机</span>
              </td>
            </tr>
          </tbody>
        </table>
        <div v-else class="text-sm text-on-surface-variant py-8 text-center">
          {{ servers.length === 0 ? '请先录入服务器' : '暂无暴露端口：请先在「中间件编排」中添加实例并暴露端口' }}
        </div>
      </div>
    </section>

    <!-- 规则明细 -->
    <section>
      <div class="flex justify-between items-center mb-3">
        <h3 class="text-sm font-headline font-bold text-primary uppercase tracking-widest">
          访问规则明细
        </h3>
        <button
          class="px-4 py-1 rounded-lg text-xs font-bold border border-primary/50 text-primary hover:bg-primary/10 transition-colors"
          @click="openCreate"
        >
          + 添加规则
        </button>
      </div>
      <el-table :data="rules" empty-text="暂无规则（也可直接点击上方矩阵生成）" size="small">
        <el-table-column label="来源服务器" min-width="120">
          <template #default="{ row }">{{ serverName(row.fromServerId) }}</template>
        </el-table-column>
        <el-table-column label="目标服务器" min-width="120">
          <template #default="{ row }">{{ serverName(row.toServerId) }}</template>
        </el-table-column>
        <el-table-column label="目标端口" width="100">
          <template #default="{ row }">
            <span class="font-mono text-primary">{{ row.toPort }}</span>
          </template>
        </el-table-column>
        <el-table-column prop="protocol" width="70" label="协议" />
        <el-table-column prop="description" label="说明" min-width="160" show-overflow-tooltip />
        <el-table-column label="操作" width="120" align="center">
          <template #default="{ row }">
            <el-button link type="primary" size="small" @click="openEdit(row)">编辑</el-button>
            <el-button link type="danger" size="small" @click="handleDelete(row)">删除</el-button>
          </template>
        </el-table-column>
      </el-table>
    </section>

    <el-dialog v-model="dialogOpen" :title="editingRuleId ? '编辑访问规则' : '添加访问规则'" width="520">
      <el-form label-width="100px" @submit.prevent>
        <el-form-item label="来源服务器">
          <el-select v-model="form.fromServerId">
            <el-option v-for="s in servers" :key="s.id" :value="s.id" :label="s.name" />
          </el-select>
        </el-form-item>
        <el-form-item label="目标服务器">
          <el-select v-model="form.toServerId">
            <el-option v-for="s in servers" :key="s.id" :value="s.id" :label="s.name" />
          </el-select>
        </el-form-item>
        <el-form-item label="目标端口">
          <el-select v-model="form.toPort" :disabled="!form.toServerId">
            <el-option
              v-for="o in targetPortOptions(form.toServerId)"
              :key="o.value"
              :value="o.value"
              :label="o.label"
            />
          </el-select>
        </el-form-item>
        <el-form-item label="协议">
          <el-select v-model="form.protocol">
            <el-option value="tcp" label="tcp" />
            <el-option value="udp" label="udp" />
          </el-select>
        </el-form-item>
        <el-form-item label="说明">
          <el-input v-model="form.description" placeholder="如：应用访问 MySQL" maxlength="80" />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="dialogOpen = false">取消</el-button>
        <el-button type="primary" @click="handleSave">确定</el-button>
      </template>
    </el-dialog>
  </div>
</template>
