<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { ElMessage } from 'element-plus'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { useProjectStore } from '@/stores/project'
import { backend, toAppError } from '@/api/backend'
import { validateProject, hasBlockingErrors, type ValidationIssue } from '@/utils/validate'
import type { MiddlewareTemplate } from '@/types/catalog'
import type { BuildResult, BuildProgressEvent } from '@/types/build'

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

async function handleBuild() {
  if (!store.project) return
  const issues: ValidationIssue[] = validateProject(store.project, templates.value)
  if (hasBlockingErrors(issues)) {
    ElMessage.error(`存在 ${issues.filter((i) => i.level === 'error').length} 个校验错误，请先在「校验」页签处理`)
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
    result.value = await backend.buildOfflinePackage(store.project, autoPull.value)
    ElMessage.success(`构建完成: ${result.value.buildId}`)
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
    </section>

    <!-- 进度日志 -->
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
