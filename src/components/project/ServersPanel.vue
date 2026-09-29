<script setup lang="ts">
import { ref, computed } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { useProjectStore } from '@/stores/project'
import { ARCH_OPTIONS, OS_FAMILY_OPTIONS, type ServerInfo } from '@/types/project'
import { genId } from '@/utils/id'

const store = useProjectStore()

const dialogOpen = ref(false)
const editingIndex = ref(-1) // -1 = 新增
const form = ref<ServerInfo>(emptyForm())

function emptyForm(): ServerInfo {
  return {
    id: '',
    name: '',
    hostname: '',
    ip: '',
    osFamily: 'kylin',
    osVersion: 'V10 SP3',
    arch: 'amd64',
    bits: 64,
    cpuCores: 8,
    memoryGb: 32,
    diskSystemGb: 100,
    diskDataGb: 500,
    dockerVersion: '27.5.1',
    dockerDataRoot: '/data/docker',
    deployBaseDir: '/opt/stack'
  }
}

const servers = computed(() => store.project?.servers ?? [])
const editingTitle = computed(() => (editingIndex.value >= 0 ? '编辑服务器' : '新增服务器'))

const archMap = Object.fromEntries(ARCH_OPTIONS.map((a) => [a.value, a]))
const osMap = Object.fromEntries(OS_FAMILY_OPTIONS.map((o) => [o.value, o.label]))

function archColor(arch: string): 'primary' | 'success' | 'warning' | 'info' {
  if (arch === 'amd64') return 'primary'
  if (arch === 'arm64') return 'success'
  if (arch === 'loongarch64') return 'warning'
  return 'info'
}

const IP_RE = /^(\d{1,3}\.){3}\d{1,3}$/

function validateForm(): string | null {
  const f = form.value
  if (!f.name.trim()) return '服务器名称不能为空'
  if (f.ip && !IP_RE.test(f.ip)) return `IP 格式不正确: ${f.ip}`
  if (f.cpuCores <= 0) return 'CPU 核数需大于 0'
  if (f.memoryGb <= 0) return '内存需大于 0'
  // R1：同方案内名称 / 主机名 / IP 唯一
  for (let i = 0; i < servers.value.length; i++) {
    if (i === editingIndex.value) continue
    const s = servers.value[i]
    if (s.name.trim() === f.name.trim()) return `服务器名称「${f.name}」已存在`
    if (f.hostname && s.hostname && s.hostname === f.hostname) return `主机名「${f.hostname}」已存在`
    if (f.ip && s.ip === f.ip) return `IP「${f.ip}」已存在`
  }
  return null
}

function openCreate() {
  editingIndex.value = -1
  form.value = emptyForm()
  dialogOpen.value = true
}

function openEdit(index: number) {
  editingIndex.value = index
  form.value = JSON.parse(JSON.stringify(servers.value[index]))
  dialogOpen.value = true
}

function handleSave() {
  const err = validateForm()
  if (err) {
    ElMessage.warning(err)
    return
  }
  store.mutate((p) => {
    if (editingIndex.value >= 0) {
      p.servers[editingIndex.value] = form.value
    } else {
      form.value.id = genId('srv')
      p.servers.push(form.value)
    }
  })
  dialogOpen.value = false
  ElMessage.success(editingIndex.value >= 0 ? '已更新（保存方案后生效）' : '已添加（保存方案后生效）')
}

async function handleDelete(index: number) {
  const s = servers.value[index]
  const instCount = store.project?.instances.filter((i) => i.serverId === s.id).length ?? 0
  const ruleCount =
    store.project?.networkRules.filter(
      (r) => r.fromServerId === s.id || r.toServerId === s.id
    ).length ?? 0
  const extra =
    instCount || ruleCount
      ? `该服务器上有 ${instCount} 个中间件实例、${ruleCount} 条访问规则，将一并删除。`
      : ''
  try {
    await ElMessageBox.confirm(
      `确定删除服务器「${s.name}」？${extra}该操作需保存方案后生效。`,
      '删除确认',
      { type: 'warning', confirmButtonText: '删除', cancelButtonText: '取消' }
    )
  } catch {
    return
  }
  store.mutate((p) => {
    p.servers.splice(index, 1)
    p.instances = p.instances.filter((i) => i.serverId !== s.id)
    p.networkRules = p.networkRules.filter(
      (r) => r.fromServerId !== s.id && r.toServerId !== s.id
    )
  })
  ElMessage.success('已删除（保存方案后生效）')
}
</script>

<template>
  <div>
    <div class="flex justify-between items-center mb-4">
      <p class="text-on-surface-variant text-sm">
        共 <span class="text-primary font-mono">{{ servers.length }}</span> 台服务器
      </p>
      <button
        class="px-5 py-1.5 rounded-xl font-headline font-bold text-xs uppercase tracking-wider transition-all bg-gradient-to-br from-primary to-primary-dim text-on-primary hover:opacity-90 active:scale-95"
        @click="openCreate"
      >
        + 新增服务器
      </button>
    </div>

    <el-table :data="servers" empty-text="暂无服务器，点击右上角「新增服务器」录入">
      <el-table-column prop="name" label="名称" min-width="120" />
      <el-table-column label="IP / 主机名" min-width="140">
        <template #default="{ row }">
          <span class="font-mono text-xs">{{ row.ip || '—' }}</span>
          <span class="text-on-surface-variant/50 text-xs"> {{ row.hostname }}</span>
        </template>
      </el-table-column>
      <el-table-column label="架构" width="120">
        <template #default="{ row }">
          <el-tag size="small" :type="archColor(row.arch)">{{ row.arch }}</el-tag>
        </template>
      </el-table-column>
      <el-table-column label="操作系统" min-width="130">
        <template #default="{ row }">
          <span class="text-sm">{{ osMap[row.osFamily] || row.osFamily }} {{ row.osVersion }}</span>
        </template>
      </el-table-column>
      <el-table-column label="配置 (核/GB/GB)" width="130" align="center">
        <template #default="{ row }">
          <span class="font-mono text-xs"
            >{{ row.cpuCores }}c / {{ row.memoryGb }}G / {{ row.diskDataGb }}G</span
          >
        </template>
      </el-table-column>
      <el-table-column label="Docker" width="90" align="center">
        <template #default="{ row }">
          <span class="font-mono text-xs">{{ row.dockerVersion }}</span>
        </template>
      </el-table-column>
      <el-table-column label="实例" width="60" align="center">
        <template #default="{ row }">
          <el-tag size="small" type="info">{{
            store.project?.instances.filter((i) => i.serverId === row.id).length ?? 0
          }}</el-tag>
        </template>
      </el-table-column>
      <el-table-column label="操作" width="120" align="center">
        <template #default="{ $index }">
          <el-button link type="primary" size="small" @click="openEdit($index)">编辑</el-button>
          <el-button link type="danger" size="small" @click="handleDelete($index)">删除</el-button>
        </template>
      </el-table-column>
    </el-table>

    <el-dialog v-model="dialogOpen" :title="editingTitle" width="640" top="6vh">
      <el-form label-width="120px" @submit.prevent>
        <div class="grid grid-cols-2 gap-x-6">
          <el-form-item label="名称" required>
            <el-input v-model="form.name" placeholder="如：应用服务器01" maxlength="40" />
          </el-form-item>
          <el-form-item label="IP 地址">
            <el-input v-model="form.ip" placeholder="如：10.10.1.11" />
          </el-form-item>
          <el-form-item label="主机名">
            <el-input v-model="form.hostname" placeholder="如：app-01" />
          </el-form-item>
          <el-form-item label="CPU 架构" required>
            <el-select v-model="form.arch">
              <el-option
                v-for="a in ARCH_OPTIONS"
                :key="a.value"
                :value="a.value"
                :label="`${a.label}（${a.hint}）`"
              />
            </el-select>
          </el-form-item>
          <el-form-item label="操作系统" required>
            <el-select v-model="form.osFamily">
              <el-option v-for="o in OS_FAMILY_OPTIONS" :key="o.value" :value="o.value" :label="o.label" />
            </el-select>
          </el-form-item>
          <el-form-item label="系统版本">
            <el-input v-model="form.osVersion" placeholder="如：V10 SP3 / Server 20" />
          </el-form-item>
          <el-form-item label="CPU 核数">
            <el-input-number v-model="form.cpuCores" :min="1" :max="1024" />
          </el-form-item>
          <el-form-item label="内存 (GB)">
            <el-input-number v-model="form.memoryGb" :min="1" :max="4096" />
          </el-form-item>
          <el-form-item label="系统盘 (GB)">
            <el-input-number v-model="form.diskSystemGb" :min="10" :max="8192" />
          </el-form-item>
          <el-form-item label="数据盘 (GB)">
            <el-input-number v-model="form.diskDataGb" :min="10" :max="32768" />
          </el-form-item>
          <el-form-item label="Docker 版本">
            <el-input v-model="form.dockerVersion" placeholder="27.5.1" />
          </el-form-item>
          <el-form-item label="Docker 数据目录">
            <el-input v-model="form.dockerDataRoot" placeholder="/data/docker" />
          </el-form-item>
          <el-form-item label="部署目录" class="col-span-2">
            <el-input v-model="form.deployBaseDir" placeholder="/opt/stack" />
            <div class="text-xs text-on-surface-variant mt-1">
              离线包在服务器上的解压与运行目录（deploy.sh 将安装到此目录）
            </div>
          </el-form-item>
        </div>
      </el-form>
      <template #footer>
        <el-button @click="dialogOpen = false">取消</el-button>
        <el-button type="primary" @click="handleSave">确定</el-button>
      </template>
    </el-dialog>
  </div>
</template>
