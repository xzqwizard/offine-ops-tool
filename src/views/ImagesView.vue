<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { backend, toAppError } from '@/api/backend'
import type { EngineStatus, CachedImage, ImagePullEvent } from '@/types/images'
import { ARCH_OPTIONS } from '@/types/project'

import { useProjectStore } from '@/stores/project'
const projectStore = useProjectStore()
const projectRegistry = computed(() => projectStore.project?.registry ?? undefined)

const engine = ref<EngineStatus | null>(null)

// 拉取进度（百分比/速度，来自后端临时文件监视）
const pullPercent = ref<number | null>(null)
const pullSpeed = ref(0)
const pullBytes = ref(0)
const pullTotal = ref(0)
const installing = ref(false)
const installLogs = ref<string[]>([])
const cache = ref<CachedImage[]>([])
const cacheUsage = ref<Map<string, string[]>>(new Map())
const purging = ref(false)
const cacheLoading = ref(false)

// 手动拉取表单
const pullForm = ref({ image: '', arch: 'amd64' })
const pulling = ref(false)
const pullLogs = ref<string[]>([])

let unlistenPull: UnlistenFn | null = null
let unlistenProgress: UnlistenFn | null = null
let disposed = false

onMounted(async () => {
  await Promise.all([refreshEngine(), refreshCache()])
  try {
    const fn = await listen<ImagePullEvent>('image-pull', (e) => {
      pullLogs.value.push(e.payload.detail)
    })
    const fnProgress = await listen<{
      reference: string
      arch: string
      bytes: number
      total: number
      percent: number
      speedBps: number
    }>('image-pull-progress', (e) => {
      pullPercent.value = e.payload.percent
      pullSpeed.value = e.payload.speedBps
      pullBytes.value = e.payload.bytes
      pullTotal.value = e.payload.total
    })
    if (disposed) {
      fn()
      fnProgress()
    } else {
      unlistenPull = fn
      unlistenProgress = fnProgress
    }
  } catch {
    /* 非 Tauri 环境忽略 */
  }
})

onUnmounted(() => {
  disposed = true
  unlistenPull?.()
  unlistenProgress?.()
})

async function refreshEngine() {
  try {
    engine.value = await backend.engineStatus()
  } catch (e) {
    ElMessage.error(`引擎状态读取失败: ${toAppError(e).message}`)
  }
}

async function refreshCache() {
  cacheLoading.value = true
  try {
    ;[cache.value] = await Promise.all([backend.listImageCache()])
    backend
      .analyzeCacheUsage()
      .then((usage) => {
        cacheUsage.value = new Map(usage.map((u) => [u.file, u.referencedBy]))
      })
      .catch(() => {})
  } catch (e) {
    ElMessage.error(`缓存列表读取失败: ${toAppError(e).message}`)
  } finally {
    cacheLoading.value = false
  }
}

async function installEngine(force = false) {
  installing.value = true
  installLogs.value = []
  try {
    const un = await listen<{ step: string; detail: string }>('engine-install', (e) => {
      installLogs.value.push(`[${e.payload.step}] ${e.payload.detail}`)
    })
    try {
      engine.value = await backend.engineInstall(force)
      ElMessage.success(`crane ${engine.value.version ?? ''} 安装成功`)
    } finally {
      un()
    }
  } catch (e) {
    ElMessage.error(`安装失败: ${toAppError(e).message}`)
  } finally {
    installing.value = false
  }
}

async function handlePull() {
  const { image, arch } = pullForm.value
  if (!image.trim()) {
    ElMessage.warning('请输入镜像引用，如 mysql:8.0.42')
    return
  }
  pulling.value = true
  pullLogs.value = []
  pullPercent.value = null
  pullSpeed.value = 0
  pullBytes.value = 0
  pullTotal.value = 0
  try {
    const r = await backend.pullImage(image.trim(), arch, projectRegistry)
    if (r.cached) {
      pullLogs.value.push(`缓存命中: ${r.cacheFile}`)
    }
    ElMessage.success(
      r.cached ? '已存在于缓存' : `拉取完成（${r.source}，${(r.sizeBytes / 1048576).toFixed(1)} MB）`
    )
    await refreshCache()
  } catch (e) {
    ElMessage.error(`拉取失败: ${toAppError(e).message}`)
  } finally {
    pulling.value = false
    pullPercent.value = null
  }
}

function fmtSpeed(bps: number): string {
  if (bps >= 1048576) return `${(bps / 1048576).toFixed(1)} MB/s`
  if (bps >= 1024) return `${(bps / 1024).toFixed(0)} KB/s`
  return `${bps} B/s`
}

async function handlePurgeUnref() {
  const unrefCount = cache.value.filter(
    (c) => !(cacheUsage.value.get(c.file)?.length ?? 0)
  ).length
  if (!unrefCount) {
    ElMessage.info('没有未被引用的缓存镜像')
    return
  }
  try {
    await ElMessageBox.confirm(
      `清理 ${unrefCount} 个未被任何方案引用的缓存镜像？`,
      '一键清理',
      { type: 'warning', confirmButtonText: '清理', cancelButtonText: '取消' }
    )
  } catch {
    return
  }
  purging.value = true
  try {
    const [count, freed, errors] = await backend.purgeUnrefCache()
    let msg = `已清理 ${count} 项，释放 ${(freed / 1048576).toFixed(1)} MB`
    if (errors.length) msg += `；${errors.length} 项失败（可能被占用）`
    ElMessage.success(msg)
    await refreshCache()
  } catch (e) {
    ElMessage.error(`清理失败: ${toAppError(e).message}`)
  } finally {
    purging.value = false
  }
}

function refLabels(c: CachedImage): string {
  return cacheUsage.value.get(c.file)?.join('、') ?? '—'
}

async function handleDelete(c: CachedImage) {
  try {
    await ElMessageBox.confirm(
      `删除缓存镜像 ${c.reference} (${c.platform})？磁盘将释放 ${(c.sizeBytes / 1048576).toFixed(1)} MB`,
      '删除确认',
      { type: 'warning', confirmButtonText: '删除', cancelButtonText: '取消' }
    )
  } catch {
    return
  }
  try {
    await backend.deleteCachedImage(c.file)
    ElMessage.success('已删除')
    await refreshCache()
  } catch (e) {
    ElMessage.error(`删除失败: ${toAppError(e).message}`)
  }
}

const totalSize = computed(() => cache.value.reduce((s, c) => s + c.sizeBytes, 0))

function fmtSize(bytes: number): string {
  if (bytes >= 1073741824) return `${(bytes / 1073741824).toFixed(2)} GB`
  if (bytes >= 1048576) return `${(bytes / 1048576).toFixed(1)} MB`
  return `${(bytes / 1024).toFixed(0)} KB`
}

function fmtTime(iso: string): string {
  return iso ? iso.replace('T', ' ').replace(/([+-]\d{2}:\d{2}|Z)$/, '') : '—'
}
</script>

<template>
  <div class="max-w-5xl mx-auto flex flex-col gap-6">
    <h1 class="font-headline text-2xl font-bold tracking-tight">镜像库</h1>

    <!-- 引擎状态 -->
    <section class="bg-surface-container-low rounded-xl border border-outline-variant p-5">
      <div class="flex items-center justify-between">
        <div class="flex items-center gap-3">
          <span class="material-symbols-outlined text-primary text-2xl">precision_manufacturing</span>
          <div>
            <div class="text-sm font-medium">镜像引擎 crane</div>
            <div class="text-xs text-on-surface-variant font-mono">
              <template v-if="engine?.installed">
                {{ engine.version }} · {{ engine.path }}
              </template>
              <template v-else>未安装（拉取镜像前需先安装）</template>
            </div>
          </div>
        </div>
        <div class="flex gap-2">
          <button
            v-if="engine?.installed"
            class="px-4 py-1.5 rounded-lg text-xs font-bold border border-outline-variant text-on-surface-variant hover:border-primary hover:text-primary transition-colors"
            :disabled="installing"
            @click="installEngine(true)"
          >
            检查更新
          </button>
          <button
            class="px-5 py-1.5 rounded-xl font-headline font-bold text-xs uppercase tracking-wider transition-all bg-gradient-to-br from-primary to-primary-dim text-on-primary hover:opacity-90 active:scale-95 disabled:opacity-30"
            :disabled="installing"
            @click="installEngine(false)"
          >
            {{ installing ? '安装中…' : engine?.installed ? '重新下载' : '下载安装' }}
          </button>
        </div>
      </div>
      <div
        v-if="installLogs.length"
        class="mt-3 bg-surface-dim rounded-lg p-3 font-mono text-xs leading-relaxed overflow-y-auto max-h-32"
      >
        <div v-for="(l, i) in installLogs" :key="i">{{ l }}</div>
      </div>
      <div class="text-[10px] text-on-surface-variant/50 mt-2">
        从 go-containerregistry 官方 GitHub Release 下载（sha256 校验）；外网访问走「设置 → 网络代理」
      </div>
    </section>

    <!-- 手动拉取 -->
    <section class="bg-surface-container-low rounded-xl border border-outline-variant p-5">
      <h3 class="text-sm font-headline font-bold text-primary uppercase tracking-widest mb-4">
        拉取镜像到缓存
      </h3>
      <div class="flex gap-3 items-start">
        <div class="flex-1">
          <el-input
            v-model="pullForm.image"
            placeholder="镜像引用，如 mysql:8.0.42 或 registry.example.cn/gov/app:2.3.1"
            class="font-mono"
            :disabled="pulling"
            @keyup.enter="handlePull"
          />
          <div class="text-[10px] text-on-surface-variant/50 mt-1">
            按目标服务器架构拉取（docker.io 引用自动按镜像源顺序回退）
          </div>
        </div>
        <el-select v-model="pullForm.arch" class="w-44" :disabled="pulling">
          <el-option
            v-for="a in ARCH_OPTIONS.filter((x) => ['amd64', 'arm64', 'loongarch64'].includes(x.value))"
            :key="a.value"
            :value="a.value"
            :label="`${a.value}（${a.hint}）`"
          />
        </el-select>
        <button
          class="px-6 py-2 rounded-xl font-headline font-bold text-xs uppercase tracking-wider transition-all bg-gradient-to-br from-primary to-primary-dim text-on-primary hover:opacity-90 active:scale-95 disabled:opacity-30"
          :disabled="pulling || !engine?.installed"
          @click="handlePull"
        >
          {{ pulling ? '拉取中…' : '拉取' }}
        </button>
      </div>
      <!-- 拉取进度（百分比/速度/已下载量） -->
      <div v-if="pulling && pullPercent !== null" class="mt-3">
        <div class="flex items-center justify-between text-xs font-mono mb-1">
          <span class="text-primary">{{ pullPercent.toFixed(1) }}%</span>
          <span class="text-on-surface-variant">
            {{ (pullBytes / 1048576).toFixed(1) }} / {{ (pullTotal / 1048576).toFixed(1) }} MB ·
            {{ fmtSpeed(pullSpeed) }}
          </span>
        </div>
        <el-progress :percentage="Math.floor(pullPercent)" :stroke-width="10" :show-text="false" />
      </div>
      <div
        v-if="pullLogs.length"
        class="mt-3 bg-surface-dim rounded-lg p-3 font-mono text-xs leading-relaxed overflow-y-auto max-h-32"
      >
        <div v-for="(l, i) in pullLogs" :key="i">{{ l }}</div>
      </div>
    </section>

    <!-- 缓存列表 -->
    <section>
      <div class="flex justify-between items-center mb-3">
        <h3 class="text-sm font-headline font-bold text-primary uppercase tracking-widest">
          本地缓存（{{ cache.length }} 个 · {{ fmtSize(totalSize) }}）
        </h3>
        <div class="flex gap-3">
          <button
            class="text-xs text-warning hover:opacity-70 transition-opacity"
            :disabled="purging"
            @click="handlePurgeUnref"
          >
            {{ purging ? '清理中…' : '一键清理未引用' }}
          </button>
          <button
            class="text-xs text-on-surface-variant hover:text-primary transition-colors"
            @click="refreshCache"
          >
            刷新
          </button>
        </div>
      </div>
      <div class="bg-surface-container-low rounded-xl border border-outline-variant">
        <el-table
          :data="cache"
          v-loading="cacheLoading"
          empty-text="暂无缓存镜像（拉取后自动进入缓存，构建时复用）"
          size="small"
        >
          <el-table-column label="镜像" min-width="220" show-overflow-tooltip>
            <template #default="{ row }">
              <span class="font-mono text-xs">{{ row.reference }}</span>
            </template>
          </el-table-column>
          <el-table-column label="平台" width="120">
            <template #default="{ row }">
              <el-tag size="small" class="font-mono">{{ row.platform }}</el-tag>
            </template>
          </el-table-column>
          <el-table-column label="大小" width="90" align="right">
            <template #default="{ row }">
              <span class="font-mono text-xs">{{ fmtSize(row.sizeBytes) }}</span>
            </template>
          </el-table-column>
          <el-table-column label="被方案引用" min-width="140" show-overflow-tooltip>
            <template #default="{ row }">
              <span v-if="(cacheUsage.get(row.file)?.length ?? 0)" class="text-xs">{{
                refLabels(row)
              }}</span>
              <span v-else class="text-xs text-warning">未引用</span>
            </template>
          </el-table-column>
          <el-table-column label="拉取时间" width="150">
            <template #default="{ row }">
              <span class="font-mono text-xs text-on-surface-variant">{{ fmtTime(row.pulledAt) }}</span>
            </template>
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
