<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { useRouter } from 'vue-router'
import { useBuildTaskStore } from '@/stores/buildTask'
import { formatTime } from '@/utils/time'
import { useProjectStore } from '@/stores/project'
import { backend, toAppError } from '@/api/backend'
import { hasBlockingErrors } from '@/utils/validate'
import type { MiddlewareTemplate } from '@/types/catalog'
import type { BuildHistoryEntry } from '@/types/buildHistory'

const store = useProjectStore()
const router = useRouter()
const templates = ref<MiddlewareTemplate[]>([])
const artifactRoot = ref('')
const tasks = useBuildTaskStore()
const building = computed(() => tasks.busy)
const logs = computed(() => tasks.records[store.project?.id ?? '']?.logs ?? [])
const result = computed(() => tasks.records[store.project?.id ?? '']?.result ?? null)
onMounted(async () => {
  try { templates.value = (await backend.listCatalog()).templates; artifactRoot.value = (await backend.getStorageInfo()).artifactRoot }
  catch (e) { ElMessage.error(`初始化失败: ${toAppError(e).message}`) }
})

const format = computed({
  get: () => store.project?.buildConfig.packageFormat ?? 'tar.gz',
  set: (v: 'dir' | 'tar.gz') => store.scheduleSave((p) => (p.buildConfig.packageFormat = v))
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
watch(() => store.project?.id, () => { baselineBuildId.value = null; upgradeMode.value = false; refreshHistory() }, { immediate: true })


async function refreshHistory() {
  const projectId = store.project?.id
  historyLoading.value = true
  try {
    const entries = await backend.listBuildHistory()
    if (projectId === store.project?.id) history.value = entries.filter(h => h.projectId === projectId)
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
      h.status === 'complete' && h.schemaVersion === 2 &&
      h.servers.length === currentProjectServers.value.length &&
      h.servers.every((bs) => currentProjectServers.value.some((s) =>
        s.id === bs.serverId && s.arch === bs.arch && s.osFamily === bs.osFamily &&
        s.osVersion === bs.osVersion && s.dockerVersion === bs.dockerVersion &&
        s.dockerDataRoot === bs.dockerDataRoot && s.deployBaseDir === bs.deployBaseDir && s.ip === bs.ip))
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

async function restoreFromBuild(entry: BuildHistoryEntry) {
  try {
    await ElMessageBox.confirm(
      `从构建 ${entry.buildId} 的快照恢复为的新方案？（当前方案不受影响）`,
      '恢复方案',
      { type: 'info', confirmButtonText: '恢复', cancelButtonText: '取消' }
    )
  } catch {
    return
  }
  try {
    const p = await backend.restoreProjectFromBuild(entry.dir)
    ElMessage.success(`已恢复为「${p.name}」`)
    router.push({ name: 'project-edit', params: { id: p.id } })
  } catch (e) {
    ElMessage.error(`恢复失败: ${toAppError(e).message}`)
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
    if (baselineBuildId.value === entry.buildId) {
      baselineBuildId.value = null
      upgradeMode.value = false
    }
    ElMessage.success('已删除')
    await refreshHistory()
  } catch (e) {
    ElMessage.error(`删除失败: ${toAppError(e).message}`)
  }
}

async function handleBuild() {
  if (!store.project || building.value) return
  // Snapshot before any asynchronous work. The global store keeps this task alive across navigation.
  const snapshot = JSON.parse(JSON.stringify(store.project))
  const baseline = upgradeMode.value ? baselineBuildId.value : null
  if (upgradeMode.value && !baseline) { ElMessage.error('请选择升级基线'); return }
  try {
    await tasks.run(snapshot, autoPull.value, baseline, () => store.flushPending())
    ElMessage.success('构建完成')
    await refreshHistory()
  } catch (e) { ElMessage.error(`构建失败: ${toAppError(e).message}`); await refreshHistory() }
}

function fmtSize(bytes: number): string {
  if (bytes >= 1073741824) return `${(bytes / 1073741824).toFixed(2)} GB`
  if (bytes >= 1048576) return `${(bytes / 1048576).toFixed(1)} MB`
  return `${(bytes / 1024).toFixed(0)} KB`
}

const fmtTime = formatTime

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
            默认使用快速 gzip；开启后使用标准压缩，耗时更长
          </div>
        </div>
        <div>
          <div class="text-xs text-on-surface-variant mb-1.5">缺失镜像自动拉取</div>
          <el-switch v-model="autoPull" />
          <div class="text-[10px] text-on-surface-variant/50 mt-1">
            开启时在线解析固定 digest；关闭时仅用验证过的缓存（需在「镜像库」安装 crane 引擎）
          </div>
        </div>
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
          现场：解压到独立目录 → bash scripts/upgrade.sh --target 已部署目录（冷备份数据及旧镜像，失败自动回滚）
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
          <template #default="{ row }">{{ row.servers.reduce((s: number, x: any) => s + x.changedImageCount, 0) }}</template>
        </el-table-column>
        <el-table-column label="体积" width="90" align="right">
          <template #default="{ row }">
            <span class="font-mono text-xs">{{ fmtSize(row.totalSizeBytes) }}</span>
          </template>
        </el-table-column>
        <el-table-column label="操作" width="180" align="center">
          <template #default="{ row }">
            <el-button link type="primary" size="small" @click="openBuildDir(row.dir)">打开目录</el-button>
            <el-button link type="primary" size="small" :disabled="!row.hasSnapshot" @click="restoreFromBuild(row)">恢复方案</el-button>
            <el-button
              v-if="baselineOptions.some(h => h.buildId === row.buildId)"
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
          <div class="text-xs text-on-surface-variant mt-1">内嵌镜像 {{ s.images.filter(i => i.packed).length }} 个；当前有效镜像 {{ s.images.length }} 个</div>
          <div v-for="w in s.warnings" :key="w" class="text-xs text-warning mt-1">⚠ {{ w }}</div>
        </div>
      </div>
    </section>
  </div>
</template>
