<script setup lang="ts">
import { ref, computed, watch, onMounted } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { open as openFileDialog, save as saveFileDialog } from '@tauri-apps/plugin-dialog'
import { backend, toAppError } from '@/api/backend'
import type {
  DockerPkgEntry,
  ComposePluginStatus
} from '@/types/dockerPkgs'
import { OFFICIAL_DOCKER_ARCHES } from '@/types/project'
import { useTaskStore } from '@/stores/tasks'
const tasks = useTaskStore()

const pkgs = ref<DockerPkgEntry[]>([])
const composePlugins = ref<ComposePluginStatus[]>([])
const loading = ref(false)

// 导入
const importArch = ref('amd64')
const importing = computed(() => tasks.active.some(t => t.key === 'docker-material'))

// 在线下载（版本从官方源拉取列表选择）
const downloadArch = ref('amd64')
const downloadVersion = ref('')
const dockerVersions = ref<string[]>([])
const versionsLoading = ref(false)
const versionsError = ref('')
const downloading = importing
const downloadLogs = computed(() => tasks.latest('docker-material')?.logs ?? [])
let versionsSeq = 0

onMounted(async () => {
  loadDockerVersions(downloadArch.value)
  await refresh()
})
watch(() => tasks.latest('docker-material')?.status, status => { if (status === 'complete') refresh() })

// 切换架构自动刷新官方版本列表，默认选最新
watch(downloadArch, (arch) => {
  downloadVersion.value = ''
  loadDockerVersions(arch)
})

async function loadDockerVersions(arch: string) {
  const seq = ++versionsSeq
  if (!OFFICIAL_DOCKER_ARCHES.includes(arch)) {
    dockerVersions.value = []
    versionsError.value = ''
    versionsLoading.value = false
    return
  }
  versionsLoading.value = true
  versionsError.value = ''
  try {
    const list = await backend.listDockerVersions(arch)
    if (seq !== versionsSeq || downloadArch.value !== arch) return
    dockerVersions.value = list
    if (list.length && !downloadVersion.value) {
      downloadVersion.value = list[0]
    }
  } catch (e) {
    if (seq !== versionsSeq) return
    dockerVersions.value = []
    versionsError.value = toAppError(e).message
  } finally {
    if (seq === versionsSeq) versionsLoading.value = false
  }
}

async function refresh() {
  loading.value = true
  try {
    ;[pkgs.value, composePlugins.value] = await Promise.all([
      backend.listDockerPkgs(),
      backend.listComposePlugins()
    ])
  } catch (e) {
    ElMessage.error(`读取安装包库失败: ${toAppError(e).message}`)
  } finally {
    loading.value = false
  }
}

const inTauri = '__TAURI_INTERNALS__' in window

async function handleImport() {
  if (importing.value) return
  if (!inTauri) {
    ElMessage.warning('文件选择仅支持桌面应用环境')
    return
  }
  try {
    const files = await openFileDialog({
      multiple: true,
      title: '选择 Docker 安装包文件（rpm/deb/docker-*.tgz/docker-compose-*）',
      filters: [
        {
          name: 'Docker 安装包',
          extensions: ['rpm', 'deb', 'tgz']
        },
        { name: '所有文件', extensions: ['*'] }
      ]
    })
    const paths = (Array.isArray(files) ? files : files ? [files] : []).filter(Boolean) as string[]
    if (!paths.length) return
    const arch = importArch.value
    try {
      const result = await tasks.run('docker-material', '导入 Docker 材料', { paths, arch }, ['docker-pkg-download'], id => backend.importDockerPkgs(paths, arch, id))
      const parts: string[] = []
      if (result.createdDirs.length) parts.push(`新增 ${result.createdDirs.length} 组`)
      if (result.composeInstalled.length) parts.push(`compose 插件 ${result.composeInstalled.length} 个`)
      if (result.skipped.length) parts.push(`跳过 ${result.skipped.length} 个`)
      ElMessage.success(`导入完成：${parts.join('，') || '无变化'}`)
      if (result.skipped.length) ElMessage.warning(result.skipped.join('；'))
      await refresh()
    } catch (e) {
      ElMessage.error(`导入失败: ${toAppError(e).message}`)
    }
  } catch (e) {
    ElMessage.error(`打开文件选择失败: ${toAppError(e).message}`)
  }
}

async function handleDownloadStatic() {
  if (downloading.value) return
  const version = downloadVersion.value.trim()
  if (!version) {
    ElMessage.warning('请输入 Docker 版本（如 27.5.1）')
    return
  }
  const arch = downloadArch.value
  try {
    const entry = await tasks.run('docker-material', `下载 Docker ${version}/${arch}`, { arch, version }, ['docker-pkg-download'], id => backend.downloadDockerStatic(arch, version, id))
    ElMessage.success(`已下载：${entry.id}（${(entry.sizeBytes / 1048576).toFixed(1)} MB）`)
    await refresh()
  } catch (e) {
    ElMessage.error(`下载失败: ${toAppError(e).message}`)
  }
}

async function handleDownloadCompose(arch: string) {
  if (downloading.value) return
  try {
    await tasks.run('docker-material', `下载 Compose/${arch}`, { arch }, ['docker-pkg-download'], id => backend.downloadComposePlugin(arch, id))
    ElMessage.success(`compose 插件（${arch}）已就绪`)
    await refresh()
  } catch (e) {
    ElMessage.error(`下载失败: ${toAppError(e).message}`)
  }
}

async function handleDelete(entry: DockerPkgEntry) {
  try {
    await ElMessageBox.confirm(
      `删除安装包组 ${entry.id}（${(entry.sizeBytes / 1048576).toFixed(1)} MB）？`,
      '删除确认',
      { type: 'warning', confirmButtonText: '删除', cancelButtonText: '取消' }
    )
  } catch {
    return
  }
  try {
    await backend.deleteDockerPkg(entry.dir)
    ElMessage.success('已删除')
    await refresh()
  } catch (e) {
    ElMessage.error(`删除失败: ${toAppError(e).message}`)
  }
}

function kindLabel(kind: string): string {
  if (kind === 'rpm') return 'RPM 包'
  if (kind === 'deb') return 'DEB 包'
  if (kind === 'static') return '官方静态包'
  return kind
}

function fmtSize(bytes: number): string {
  if (bytes >= 1073741824) return `${(bytes / 1073741824).toFixed(2)} GB`
  if (bytes >= 1048576) return `${(bytes / 1048576).toFixed(1)} MB`
  return `${(bytes / 1024).toFixed(0)} KB`
}

const totalSize = computed(() => pkgs.value.reduce((s, p) => s + p.sizeBytes, 0))
</script>

<template>
  <div class="max-w-5xl mx-auto flex flex-col gap-6" v-loading="loading">
    <h1 class="font-headline text-2xl font-bold tracking-tight">Docker 安装包库</h1>

    <!-- 导入与在线下载 -->
    <section class="bg-surface-container-low rounded-xl border border-outline-variant p-5">
      <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
        <!-- 本地导入 -->
        <div>
          <h3 class="text-sm font-headline font-bold text-primary uppercase tracking-widest mb-3">
            本地导入
          </h3>
          <div class="flex gap-2 items-center mb-2">
            <span class="text-xs text-on-surface-variant shrink-0">静态包架构：</span>
            <el-select v-model="importArch" size="small" class="w-32">
              <el-option value="amd64" label="amd64" />
              <el-option value="arm64" label="arm64" />
              <el-option value="loongarch64" label="loongarch64" />
            </el-select>
          </div>
          <button
            class="w-full px-4 py-2 rounded-lg text-xs font-bold border border-primary/50 text-primary hover:bg-primary/10 transition-colors"
            :disabled="importing"
            @click="handleImport"
          >
            {{ importing ? '导入中…' : '选择文件导入（可多选 rpm/deb/tgz/compose）' }}
          </button>
          <div class="text-[10px] text-on-surface-variant/50 mt-2 leading-relaxed">
            RPM/DEB 需同名 .pkg.json 声明 OS、兼容版本、组件及 SHA256，并核对包内元信息；
            Compose 需同名 .compose.json 声明版本、架构、大小及 SHA256；静态包按所选架构核验全部必需二进制。
          </div>
        </div>

        <!-- 在线下载 -->
        <div>
          <h3 class="text-sm font-headline font-bold text-primary uppercase tracking-widest mb-3">
            在线下载（官方源）
          </h3>
          <div class="flex gap-2 mb-2 items-center">
            <el-select v-model="downloadArch" size="small" class="w-32">
              <el-option
                v-for="a in OFFICIAL_DOCKER_ARCHES"
                :key="a"
                :value="a"
                :label="a"
              />
            </el-select>
            <el-select
              v-model="downloadVersion"
              size="small"
              filterable
              allow-create
              default-first-option
              :loading="versionsLoading"
              placeholder="从官方源获取版本列表…"
              class="flex-1"
            >
              <el-option v-for="v in dockerVersions" :key="v" :value="v" :label="v" />
            </el-select>
            <button
              class="px-3 py-1 rounded-lg text-xs font-bold bg-gradient-to-br from-primary to-primary-dim text-on-primary hover:opacity-90 transition-all shrink-0"
              :disabled="downloading || !downloadVersion"
              @click="handleDownloadStatic"
            >
              下载静态包
            </button>
          </div>
          <div v-if="versionsError" class="text-xs text-error mb-1">
            官方版本列表获取失败：{{ versionsError }}
            <button
              class="text-primary hover:underline ml-1"
              @click="loadDockerVersions(downloadArch)"
            >
              重试
            </button>
            （可直接在版本框手动输入）
          </div>
          <div class="text-[10px] text-on-surface-variant/50">
            版本列表来自 download.docker.com 官方目录（默认最新版，可输入过滤或手输）；静态包适用所有
            Linux 发行版；信创架构请从厂商源下载后本地导入
          </div>
        </div>
      </div>
      <div
        v-if="downloadLogs.length"
        class="mt-3 bg-surface-dim rounded-lg p-3 font-mono text-xs leading-relaxed overflow-y-auto max-h-32"
      >
        <div v-for="(l, i) in downloadLogs" :key="i">{{ l }}</div>
      </div>
    </section>

    <!-- compose 插件 -->
    <section class="bg-surface-container-low rounded-xl border border-outline-variant p-5">
      <h3 class="text-sm font-headline font-bold text-primary uppercase tracking-widest mb-3">
        docker compose v2 插件
      </h3>
      <div class="flex flex-wrap gap-3">
        <div
          v-for="c in composePlugins"
          :key="c.arch"
          class="flex items-center gap-3 bg-surface-container rounded-lg px-4 py-2 border border-outline-variant"
        >
          <span class="font-mono text-xs w-24">{{ c.arch }}</span>
          <span v-if="c.installed" class="text-xs text-success" :title="c.source">✓ {{ c.version }}（{{ fmtSize(c.sizeBytes) }}）</span>
          <template v-else>
            <span class="text-xs text-on-surface-variant/50" :title="c.error ?? ''">{{ c.error ? '校验失败，请重新导入或下载' : '未安装' }}</span>
            <button
              v-if="OFFICIAL_DOCKER_ARCHES.includes(c.arch)"
              class="text-xs text-primary hover:underline"
              :disabled="downloading"
              @click="handleDownloadCompose(c.arch)"
            >
              在线下载
            </button>
            <span v-else class="text-[10px] text-warning">需本地导入</span>
          </template>
        </div>
      </div>
    </section>

    <!-- 安装包列表 -->
    <section>
      <div class="flex justify-between items-center mb-3">
        <h3 class="text-sm font-headline font-bold text-primary uppercase tracking-widest">
          安装包组（{{ pkgs.length }} 组 · {{ fmtSize(totalSize) }}）
        </h3>
        <button class="text-xs text-on-surface-variant hover:text-primary transition-colors" @click="refresh">
          刷新
        </button>
      </div>
      <div class="bg-surface-container-low rounded-xl border border-outline-variant">
        <el-table
          :data="pkgs"
          empty-text="暂无安装包：本地导入或在线下载后，构建时将按服务器架构/版本自动装入离线包"
          size="small"
        >
          <el-table-column label="架构" width="110">
            <template #default="{ row }">
              <el-tag size="small" :type="row.arch === 'amd64' ? 'primary' : row.arch === 'arm64' ? 'success' : 'warning'">
                {{ row.arch }}
              </el-tag>
            </template>
          </el-table-column>
          <el-table-column label="类型" width="110">
            <template #default="{ row }">{{ kindLabel(row.kind) }}</template>
          </el-table-column>
          <el-table-column prop="dockerVersion" label="Docker 版本" width="110">
            <template #default="{ row }">
              <span class="font-mono text-xs">{{ row.dockerVersion }}</span>
            </template>
          </el-table-column>
          <el-table-column label="包含文件" min-width="220" show-overflow-tooltip>
            <template #default="{ row }">
              <span class="font-mono text-xs text-on-surface-variant">{{ row.files.join('、') }}</span>
            </template>
          </el-table-column>
          <el-table-column label="大小" width="90" align="right">
            <template #default="{ row }">
              <span class="font-mono text-xs">{{ fmtSize(row.sizeBytes) }}</span>
            </template>
          </el-table-column>
          <el-table-column label="来源" min-width="160" show-overflow-tooltip>
            <template #default="{ row }">{{ Object.values(row.sources ?? {}).join('、') || '旧材料未记录来源' }}</template>
          </el-table-column>
          <el-table-column label="操作" width="70" align="center">
            <template #default="{ row }">
              <el-button link type="danger" size="small" @click="handleDelete(row)">删除</el-button>
            </template>
          </el-table-column>
        </el-table>
      </div>
    </section>
  </div>
</template>
