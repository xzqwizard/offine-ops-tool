<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { useProjectStore } from '@/stores/project'
import {
  ARCH_OPTIONS,
  OS_FAMILY_OPTIONS,
  OS_VERSION_OPTIONS,
  OFFICIAL_DOCKER_ARCHES,
  type ServerInfo
} from '@/types/project'
import { backend, toAppError } from '@/api/backend'
import { isIpv4 } from '@/utils/validate'
import { genId } from '@/utils/id'

const store = useProjectStore()

const dialogOpen = ref(false)
const editingIndex = ref(-1) // -1 = 新增
const form = ref<ServerInfo>(emptyForm())

// Docker 官方版本列表（按架构在线获取）
const dockerVersions = ref<string[]>([])
const dockerLoading = ref(false)
const dockerError = ref('')

function emptyForm(): ServerInfo {
  return {
    id: '',
    name: '',
    hostname: '',
    ip: '',
    osFamily: 'kylin',
    osVersion: OS_VERSION_OPTIONS['kylin'][0],
    arch: 'amd64',
    bits: 64,
    cpuCores: 8,
    memoryGb: 32,
    diskSystemGb: 100,
    diskDataGb: 500,
    dockerVersion: '',
    dockerDataRoot: '/data/docker',
    deployBaseDir: '/opt/stack'
  }
}

const servers = computed(() => store.project?.servers ?? [])
const editingTitle = computed(() => (editingIndex.value >= 0 ? '编辑服务器' : '新增服务器'))

const archMap = Object.fromEntries(ARCH_OPTIONS.map((a) => [a.value, a]))
const osMap = Object.fromEntries(OS_FAMILY_OPTIONS.map((o) => [o.value, o.label]))

const officialDockerArch = computed(() => OFFICIAL_DOCKER_ARCHES.includes(form.value.arch))
const osVersionOptions = computed(() => OS_VERSION_OPTIONS[form.value.osFamily] ?? [])

function archColor(arch: string): 'primary' | 'success' | 'warning' | 'info' {
  if (arch === 'amd64') return 'primary'
  if (arch === 'arm64') return 'success'
  if (arch === 'loongarch64') return 'warning'
  return 'info'
}

function validateForm(): string | null {
  const f = form.value
  if (!f.name.trim()) return '服务器名称不能为空'
  if (!f.ip.trim()) return 'IP 地址不能为空（防火墙规则/端口矩阵依赖 IP 对应）'
  if (!isIpv4(f.ip.trim())) return `IP 格式不正确: ${f.ip}`
  if (!f.dockerVersion.trim()) return 'Docker 版本不能为空（从官方列表选择或手动输入）'
  if (f.cpuCores <= 0) return 'CPU 核数需大于 0'
  if (f.memoryGb <= 0) return '内存需大于 0'
  // R1：同方案内名称 / 主机名 / IP 唯一
  for (let i = 0; i < servers.value.length; i++) {
    if (i === editingIndex.value) continue
    const s = servers.value[i]
    if (s.name.trim() === f.name.trim()) return `服务器名称「${f.name}」已存在`
    if (f.hostname && s.hostname && s.hostname === f.hostname) return `主机名「${f.hostname}」已存在`
    if (f.ip.trim() && s.ip === f.ip.trim()) return `IP「${f.ip}」已存在`
  }
  return null
}

async function loadDockerVersions(arch: string) {
  if (!OFFICIAL_DOCKER_ARCHES.includes(arch)) {
    dockerVersions.value = []
    dockerError.value = ''
    return
  }
  dockerLoading.value = true
  dockerError.value = ''
  try {
    const list = await backend.listDockerVersions(arch)
    dockerVersions.value = list
    // 仅新增模式自动选最新版；编辑模式绝不静默覆盖用户已填版本
    if (list.length && editingIndex.value < 0 && !form.value.dockerVersion) {
      form.value.dockerVersion = list[0]
    }
  } catch (e) {
    dockerVersions.value = []
    dockerError.value = toAppError(e).message
  } finally {
    dockerLoading.value = false
  }
}

watch(
  () => form.value.arch,
  (arch) => {
    if (dialogOpen.value) loadDockerVersions(arch)
  }
)

function onOsFamilyChange(family: string) {
  const list = OS_VERSION_OPTIONS[family]
  if (list?.length) form.value.osVersion = list[0]
}

function openCreate() {
  editingIndex.value = -1
  form.value = emptyForm()
  dialogOpen.value = true
  loadDockerVersions(form.value.arch)
}

function openEdit(index: number) {
  editingIndex.value = index
  form.value = JSON.parse(JSON.stringify(servers.value[index]))
  dialogOpen.value = true
  loadDockerVersions(form.value.arch)
}

async function handleSave() {
  const err = validateForm()
  if (err) {
    ElMessage.warning(err)
    return
  }
  const isEdit = editingIndex.value >= 0
  const target = form.value
  try {
    await store.commit((p) => {
      if (isEdit) {
        p.servers[editingIndex.value] = target
      } else {
        target.id = genId('srv')
        p.servers.push(target)
      }
    })
    dialogOpen.value = false
    ElMessage.success(`服务器「${target.name}」已保存`)
  } catch (e) {
    ElMessage.error(`保存失败: ${toAppError(e).message}`)
  }
}

async function handleDelete(index: number) {
  const s = servers.value[index]
  const instCount = store.project?.instances.filter((i) => i.serverId === s.id).length ?? 0
  const ruleCount =
    store.project?.networkRules.filter((r) => r.fromServerId === s.id || r.toServerId === s.id)
      .length ?? 0
  const extra =
    instCount || ruleCount
      ? `该服务器上有 ${instCount} 个中间件实例、${ruleCount} 条访问规则，将一并删除。`
      : ''
  try {
    await ElMessageBox.confirm(`确定删除服务器「${s.name}」？${extra}`, '删除确认', {
      type: 'warning',
      confirmButtonText: '删除',
      cancelButtonText: '取消'
    })
  } catch {
    return
  }
  try {
    await store.commit((p) => {
      p.servers.splice(index, 1)
      p.instances = p.instances.filter((i) => i.serverId !== s.id)
      p.networkRules = p.networkRules.filter(
        (r) => r.fromServerId !== s.id && r.toServerId !== s.id
      )
    })
    ElMessage.success('已删除并保存')
  } catch (e) {
    ElMessage.error(`删除失败: ${toAppError(e).message}`)
  }
}
</script>

<template>
  <div>
    <div class="flex justify-between items-center mb-4">
      <p class="text-on-surface-variant text-sm">
        共 <span class="text-primary font-mono">{{ servers.length }}</span> 台服务器
        <span class="text-xs text-on-surface-variant/60 ml-2">新增/修改/删除后自动保存</span>
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
      <el-table-column label="操作系统" min-width="150">
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
          <el-form-item label="IP 地址" required>
            <el-input v-model="form.ip" placeholder="如：10.10.1.11（必填）" />
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
            <el-select v-model="form.osFamily" @change="onOsFamilyChange">
              <el-option
                v-for="o in OS_FAMILY_OPTIONS"
                :key="o.value"
                :value="o.value"
                :label="o.label"
              />
            </el-select>
          </el-form-item>
          <el-form-item label="系统版本" required>
            <el-select
              v-model="form.osVersion"
              filterable
              allow-create
              default-first-option
              :placeholder="form.osVersion || '选择或输入版本'"
            >
              <el-option v-for="v in osVersionOptions" :key="v" :value="v" :label="v" />
            </el-select>
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
          <el-form-item label="Docker 版本" class="col-span-2">
            <template v-if="officialDockerArch">
              <el-select
                v-model="form.dockerVersion"
                filterable
                :loading="dockerLoading"
                placeholder="从 Docker 官方源获取版本列表…"
                class="w-full"
              >
                <el-option v-for="v in dockerVersions" :key="v" :value="v" :label="v" />
              </el-select>
              <div v-if="dockerError" class="text-xs text-error mt-1">
                官方版本获取失败：{{ dockerError }}
                <el-button link type="primary" size="small" @click="loadDockerVersions(form.arch)"
                  >重试</el-button
                >
              </div>
              <div
                v-else-if="!dockerLoading && !dockerVersions.length"
                class="text-xs text-on-surface-variant mt-1"
              >
                官方源暂无可用版本
              </div>
            </template>
            <template v-else>
              <el-input v-model="form.dockerVersion" placeholder="如：27.5.1" class="w-full" />
              <div class="text-xs text-warning mt-1">
                {{ form.arch }} 无 Docker 官方静态包，版本号需与信创源获取的安装包一致
              </div>
            </template>
          </el-form-item>
          <el-form-item label="Docker 数据目录" class="col-span-2">
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
        <el-button type="primary" @click="handleSave">确定并保存</el-button>
      </template>
    </el-dialog>
  </div>
</template>
