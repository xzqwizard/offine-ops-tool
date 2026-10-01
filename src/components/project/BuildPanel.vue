<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { useProjectStore } from '@/stores/project'
import { backend, toAppError } from '@/api/backend'
import { validateProject, hasBlockingErrors, type ValidationIssue } from '@/utils/validate'
import type { MiddlewareTemplate } from '@/types/catalog'
import type { BuildResult, BuildProgressEvent } from '@/types/build'
import type { BuildHistoryEntry } from '@/types/buildHistory'

const store = useProjectStore()
const templates = ref<MiddlewareTemplate[]>([])
const artifactRoot = ref('')
const building = ref(false)
const logs = ref<string[]>([])
const result = ref<BuildResult | null>(null)
let unlisten: UnlistenFn | null = null
let disposed = false

onMounted(async () => {
  try {
    templates.value = (await backend.listCatalog()).templates
    const storage = await backend.getStorageInfo()
    artifactRoot.value = storage.artifactRoot
  } catch (e) {
    ElMessage.error(`初始化失败: ${toAppError(e).message}`)
  }
  await refreshHistory()
  listen<BuildProgressEvent>('build-progress', (event) => {
    const { step, detail } = event.payload
    const icon =
      step === 'error' ? '✗' : step === 'done' ? '✓' : step === 'image' || step === 'package' ? '  ' : '→'
    logs.value.push(`${icon} ${detail}`)
  }).then((fn) => {
    // 竞态防护：await 期间组件可能已卸载，立即注销
    if (disposed) fn()
    else unlisten = fn
  })
})

onUnmounted(() => {
  disposed = true
  unlisten?.()
})

const format = computed({
  get: () => store.project?.buildConfig.packageFormat ?? 'tar.gz',
  set: (v: string) => store.scheduleSave((p) => (p.buildConfig.packageFormat = v))
})

const recompress = computed({
  get: () => store.project?.buildConfig.recompressImages ?? false,
  set: (v: boolean) => store.scheduleSave((p) => (p.buildConfig.recompressImages = v))
})

/** 构建时自动拉取缺失镜像（缓存未命中时在线拉取，本次构建内存态，不入方案） */
const autoPull = ref(true)

/** 升级包模式：选中基线后构建增量升级包（仅变更镜像 + upgrade.sh） */
const upgradeMode = ref(false)
const baselineBuildId = ref<string | null>(null)

// ---- 构建历史 ----
const history = ref<BuildHistoryEntry[]>([])
const historyLoading = ref(false)

async function refreshHistory() {
  historyLoading.value = true
  try {
    history.value = (await backend.listBuildHistory()).filter(
      (h) => h.projectId === store.project?.id
    )
  } catch {
    history.value = []
  } finally {
    historyLoading.value = false
  }
}

const currentProjectServers = computed(() => store.project?.servers ?? [])
/** 基线可选：完整构建且服务器集合与当前方案一致 */
const baselineOptions = computed(() =>
  history.value.filter(
    (h) =>
      h.kind === 'full' &&
      h.servers.length === currentProjectServers.value.length &&
      h.servers.every((bs) => currentProjectServers.value.some((s) => s.name === bs.name))
  )
)

function selectBaseline(id: string | null) {
  baselineBuildId.value = id
  if (id) upgradeMode.value = true
}

function onUpgradeModeChange(v: boolean) {
  if (!v) baselineBuildId.value = null
}

async function openBuildDir(dir: string) {
  try {
    await backend.openDirInExplorer(dir)
  } catch (e) {
    ElMessage.error(`打开目录失败: ${toAppError(e).message}`)
  }
}

async function deleteBuildEntry(entry: BuildHistoryEntry) {
  try {
    await ElMessageBox.confirm(
      `删除构建 ${entry.buildId}（${entry.servers.length} 个服务器包，${fmtSize(entry.totalSizeBytes)}）？`,
      '删除确认',
      { type: 'warning', confirmButtonText: '删除', cancelButtonText: '取消' }
    )
  } catch {
    return
  }
  try {
    await backend.deleteBuild(entry.dir)
    ElMessage.success('已删除')
    await refreshHistory()
  } catch (e) {
    ElMessage.error(`删除失败: ${toAppError(e).message}`)
  }
}

/** R10 预检：每台服务器需有匹配 (arch, dockerVersion) 的安装包组（rpm/deb/static 任一）
 *  且对应架构 compose 插件；否则产物到现场装不了 Docker（设计文档定为阻断级）。
 *  升级包模式跳过（目标机已具备 Docker 环境）。 */
async function checkDockerPkgs(): Promise<string[]> {
  if (upgradeMode.value) return []
  const project = store.project!
  const problems: string[] = []
  let pkgs: { id: string }[] = []
  let compose: { arch: string; installed: boolean }[] = []
  try {
    ;[pkgs, compose] = await Promise.all([backend.listDockerPkgs(), backend.listComposePlugins()])
  } catch {
    return [] // 库读取失败不阻断（后端构建时仍会告警）
  }
  for (const s of project.servers) {
    const matched = pkgs.some((p) =>
      p.id === `pkg-${s.arch}-rpm-${s.dockerVersion}` ||
      p.id === `pkg-${s.arch}-deb-${s.dockerVersion}` ||
      p.id === `pkg-${s.arch}-static-${s.dockerVersion}`
    )
    if (!matched) {
      problems.push(`服务器「${s.name}」(Docker ${s.dockerVersion}/${s.arch}) 无匹配离线安装包`)
    }
    if (!compose.some((c) => c.arch === s.arch && c.installed)) {
      problems.push(`服务器「${s.name}」缺 ${s.arch} 架构 docker compose 插件`)
    }
  }
  return problems
}

async function handleBuild() {
  if (!store.project) return
  const issues: ValidationIssue[] = validateProject(store.project, templates.value)
  if (hasBlockingErrors(issues)) {
    ElMessage.error(`存在 ${issues.filter((i) => i.level === 'error').length} 个校验错误，请先在「校验」页签处理`)
    return
  }
  const pkgProblems = await checkDockerPkgs()
  if (pkgProblems.length) {
    ElMessage.error(`Docker 离线材料不全，已阻止构建：${pkgProblems.join('；')}。请到「Docker 安装包库」导入/下载`)
    return
  }
  if (store.dirty) {
    try {
      await store.save()
    } catch (e) {
      ElMessage.error(`构建前保存方案失败: ${toAppError(e).message}`)
      return
    }
  }
  building.value = true
  logs.value = []
  result.value = null
  try {
    result.value = await backend.buildOfflinePackage(
      store.project,
      autoPull.value,
      upgradeMode.value ? baselineBuildId.value : null
    )
    ElMessage.success(`构建完成: ${result.value.buildId}`)
    await refreshHistory()
  } catch (e) {
    ElMessage.error(`构建失败: ${toAppError(e).message}`)
  } finally {
    building.value = false
  }
}

function fmtSize(bytes: number): string {
  if (bytes >= 1073741824) return `${(bytes / 1073741824).toFixed(2)} GB`
  if (bytes >= 1048576) return `${(bytes / 1048576).toFixed(1)} MB`
  return `${(bytes / 1024).toFixed(0)} KB`
}

function fmtTime(iso: string): string {
  return iso ? iso.replace('T', ' ').replace(/([+-]\d{2}:\d{2}|Z)$/, '') : '-'
}

async function copyOutputDir() {
  if (!result.value) return
  try {
    await navigator.clipboard.writeText(result.value.outputDir)
    ElMessage.success('已复制输出目录')
  } catch {
    ElMessage.warning('复制失败，请手动选择路径')
  }
}
</script>

<template>
  <div class="flex flex-col gap-5">
    <!-- 构建配置 -->
    <section class="bg-surface-container-low rounded-xl border border-outline-variant p-5">
      <h3 class="text-sm font-headline font-bold text-primary uppercase tracking-widest mb-4">
        构建配置
      </h3>
      <div class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-4 gap-5">
        <div>
          <div class="text-xs text-on-surface-variant mb-1.5">产物输出根目录</div>
          <div class="font-mono text-xs bg-surface-container rounded-lg px-3 py-2 border border-outline-variant break-all">
            {{ artifactRoot || '…' }}
          </div>
          <div class="text-[10px] text-on-surface-variant/50 mt-1">可在「设置」中修改，避免写满系统盘</div>
        </div>
        <div>
          <div class="text-xs text-on-surface-variant mb-1.5">打包格式</div>
          <el-select v-model="format">
            <el-option value="tar.gz" label="tar.gz（推荐，保留权限）" />
            <el-option value="dir" label="dir（仅目录，不打包）" />
          </el-select>
        </div>
        <div>
          <div class="text-xs text-on-surface-variant mb-1.5">镜像参与压缩</div>
          <el-switch v-model="recompress" />
          <div class="text-[10px] text-on-surface-variant/50 mt-1">
            默认关闭：docker save 层已压缩，二次压缩收益小且耗时长
          </div>
        </div>
        <div>
          <div class="text-xs text-on-surface-variant mb-1.5">缺失镜像自动拉取</div>
          <el-switch v-model="autoPull" />
          <div class="text-[10px] text-on-surface-variant/50 mt-1">
            本地 tar → 缓存 → 在线拉取（需在「镜像库」安装 crane 引擎）
          </div>
        </div>
      </div>
      <div class="flex justify-end mt-4">
        <button
          class="px-8 py-2.5 rounded-xl font-headline font-bold text-xs uppercase tracking-wider transition-all bg-gradient-to-br from-primary to-primary-dim text-on-primary hover:opacity-90 active:scale-95 disabled:opacity-30 disabled:cursor-not-allowed"
          :disabled="building"
          @click="handleBuild"
        >
          {{ building ? '构建中…' : '开始构建离线包' }}
        </button>
      </div>

      <!-- 升级包模式 -->
      <div class="mt-4 pt-4 border-t border-outline-variant flex items-center gap-4 flex-wrap">
        <el-switch v-model="upgradeMode" @change="onUpgradeModeChange" />
        <span class="text-xs text-on-surface-variant">生成增量升级包（仅变更镜像 + upgrade.sh）</span>
        <el-select
          v-if="upgradeMode"
          v-model="baselineBuildId"
          placeholder="选择基线构建"
          size="small"
          class="w-72"
          @change="selectBaseline"
        >
          <el-option
            v-for="h in baselineOptions"
            :key="h.buildId"
            :value="h.buildId"
            :label="`${h.buildId}（${fmtTime(h.generatedAt)}）`"
          />
        </el-select>
        <span v-if="upgradeMode && !baselineBuildId" class="text-xs text-warning">
          需选择基线构建（与基线比对，仅打包新增/变更的镜像）
        </span>
        <span v-if="upgradeMode && baselineBuildId" class="text-[10px] text-on-surface-variant/50">
          现场：拷入原部署目录解压 → bash scripts/upgrade.sh（自动备份旧编排，失败可回退）
        </span>
      </div>

      <div class="flex justify-end mt-4">
        <button
          class="px-8 py-2.5 rounded-xl font-headline font-bold text-xs uppercase tracking-wider transition-all bg-gradient-to-br from-primary to-primary-dim text-on-primary hover:opacity-90 active:scale-95 disabled:opacity-30 disabled:cursor-not-allowed"
          :disabled="building || (upgradeMode && !baselineBuildId)"
          @click="handleBuild"
        >
          {{
            building
              ? '构建中…'
              : upgradeMode
                ? '生成升级包'
                : '开始构建离线包'
          }}
        </button>
      </div>
    </section>

    <!-- 构建历史 -->
    <section class="mt-2">
      <div class="flex justify-between items-center mb-3">
        <h3 class="text-sm font-headline font-bold text-primary uppercase tracking-widest">
          构建历史（{{ history.length }}）
        </h3>
        <button class="text-xs text-on-surface-variant hover:text-primary transition-colors" @click="refreshHistory">
          刷新
        </button>
      </div>
      <el-table :data="history" v-loading="historyLoading" size="small" empty-text="暂无构建记录">
        <el-table-column label="构建号" width="200">
          <template #default="{ row }">
            <span class="font-mono text-xs">{{ row.buildId }}</span>
            <el-tag v-if="row.kind === 'upgrade'" size="small" type="warning" class="ml-1">升级</el-tag>
          </template>
        </el-table-column>
        <el-table-column label="时间" width="150">
          <template #default="{ row }">
            <span class="font-mono text-xs text-on-surface-variant">{{ fmtTime(row.generatedAt) }}</span>
          </template>
        </el-table-column>
        <el-table-column label="服务器" width="70" align="center">
          <template #default="{ row }">{{ row.servers.length }}</template>
        </el-table-column>
        <el-table-column label="镜像变更" width="80" align="center">
          <template #default="{ row }">{{ row.servers.reduce((s: number, x: any) => s + x.images.length, 0) }}</template>
        </el-table-column>
        <el-table-column label="体积" width="90" align="right">
          <template #default="{ row }">
            <span class="font-mono text-xs">{{ fmtSize(row.totalSizeBytes) }}</span>
          </template>
        </el-table-column>
        <el-table-column label="操作" width="180" align="center">
          <template #default="{ row }">
            <el-button link type="primary" size="small" @click="openBuildDir(row.dir)">打开目录</el-button>
            <el-button
              v-if="row.kind === 'full'"
              link
              type="primary"
              size="small"
              @click="((upgradeMode = true), selectBaseline(row.buildId))"
              >作升级基线</el-button
            >
            <el-button link type="danger" size="small" @click="deleteBuildEntry(row)">删除</el-button>
          </template>
        </el-table-column>
      </el-table>
    </section>
    <section
      v-if="logs.length"
      class="bg-surface-dim rounded-xl border border-outline-variant p-4 font-mono text-xs leading-relaxed overflow-y-auto max-h-56"
    >
      <div v-for="(line, i) in logs" :key="i" class="whitespace-pre-wrap">{{ line }}</div>
    </section>

    <!-- 构建结果 -->
    <section v-if="result">
      <div class="flex items-center justify-between mb-3">
        <h3 class="text-sm font-headline font-bold text-primary uppercase tracking-widest">
          构建结果 · {{ result.buildId }}
        </h3>
        <button
          class="text-xs text-on-surface-variant hover:text-primary font-mono transition-colors"
          @click="copyOutputDir"
        >
          复制输出目录
        </button>
      </div>
      <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
        <div
          v-for="s in result.servers"
          :key="s.dirName"
          class="bg-surface-container-low rounded-xl border border-outline-variant p-4"
        >
          <div class="flex items-center gap-2 mb-2">
            <span class="material-symbols-outlined text-primary text-xl">dns</span>
            <span class="font-medium text-sm">{{ s.name }}</span>
            <el-tag size="small" type="info">{{ s.arch }}</el-tag>
            <span class="ml-auto font-mono text-xs text-on-surface-variant">{{
              fmtSize(s.sizeBytes)
            }}</span>
          </div>
          <div class="font-mono text-[11px] text-on-surface-variant break-all">
            {{ s.packageFile ?? s.dirName + '/（目录模式）' }}
          </div>
          <div class="text-xs text-on-surface-variant mt-1">内嵌镜像 {{ s.images.length }} 个</div>
          <div v-for="w in s.warnings" :key="w" class="text-xs text-warning mt-1">⚠ {{ w }}</div>
        </div>
      </div>
    </section>
  </div>
</template>
