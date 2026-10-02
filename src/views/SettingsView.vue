<script setup lang="ts">
import { ref, reactive, onMounted, onUnmounted } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { open as openDirectory, save as saveFileDialog, open as openFileDialog } from "@tauri-apps/plugin-dialog"
import { backend, toAppError } from '@/api/backend'
import { useBuildTaskStore } from '@/stores/buildTask'
import { useProjectStore } from '@/stores/project'
import type {
  AppSettings,
  StorageInfo,
  ProxyConfig,
  ConnectivityResult,
  NetTestEvent
} from '@/types/project'

const loading = ref(true)
const loadError = ref('')
const saving = ref(false)
let settingsSave: Promise<StorageInfo> | null = null
const settings = ref<AppSettings | null>(null)
const storage = ref<StorageInfo | null>(null)
const newMirror = ref('')

// ---- 连通性测试（弹窗 + 实时进度） ----
interface TestItem {
  label: string
  state: 'pending' | 'running' | 'ok' | 'fail'
  latencyMs: number
  error: string
}
const testOpen = ref(false)
const testing = ref(false)
const testItems = reactive<TestItem[]>([])
const testProxyUsed = ref<string | null>(null)
let unlistenNetTest: UnlistenFn | null = null
let disposed = false

function labelOf(target: string): string {
  if (target.includes('www.google.com')) return '代理通道（google.com）'
  if (target.includes('auth.docker.io')) return 'Docker Hub 认证端点'
  if (target.includes('registry-1.docker.io')) return 'Docker Hub Registry'
  if (target.includes('daocloud')) return '国内镜像源（daocloud）'
  return target
}

function emptyProxy(): ProxyConfig {
  return { enabled: false, scheme: 'http', host: '127.0.0.1', port: 7890, username: null, password: null, noProxy: null }
}

const ROOT_FIELDS: { key: keyof AppSettings; label: string; effKey: keyof StorageInfo; desc: string }[] = [
  {
    key: 'projectsRoot',
    label: '方案文件',
    effKey: 'projectsRoot',
    desc: '方案 JSON 存储（KB 级，放系统盘无压力）'
  },
  {
    key: 'imageCacheRoot',
    label: '镜像缓存',
    effKey: 'imageCacheRoot',
    desc: '镜像 blob 缓存，体积可达数十 GB，建议放数据盘'
  },
  {
    key: 'dockerPkgRoot',
    label: 'Docker 安装包库',
    effKey: 'dockerPkgRoot',
    desc: 'Docker 离线安装包（GB 级/版本）'
  },
  {
    key: 'artifactRoot',
    label: '产物输出',
    effKey: 'artifactRoot',
    desc: '离线包构建输出目录，体积 ≥ 镜像总量，建议放数据盘'
  },
  { key: 'logRoot', label: '日志', effKey: 'logRoot', desc: '应用与构建日志' }
]

async function loadAll() {
  loading.value = true
  loadError.value = ''
  try {
    ;[settings.value, storage.value] = await Promise.all([
      backend.getSettings(),
      backend.getStorageInfo()
    ])
    if (!settings.value!.proxy) settings.value!.proxy = emptyProxy()
  } catch (e) {
    loadError.value = toAppError(e).message
  } finally {
    loading.value = false
  }
}

onMounted(async () => {
  await loadAll()
  try {
    const fn = await listen<NetTestEvent>('net-test', (e) => {
      const { target, state, result } = e.payload
      const item = testItems.find((t) => t.label === labelOf(target))
      if (!item) return
      if (state === 'running') {
        item.state = 'running'
      } else if (result) {
        item.state = result.ok ? 'ok' : 'fail'
        item.latencyMs = result.latencyMs
        item.error = result.ok ? '' : result.error || '不可达'
      }
    })
    // 竞态防护：await 期间组件可能已卸载，立即注销
    if (disposed) fn()
    else unlistenNetTest = fn
  } catch {
    /* 非 Tauri 环境忽略 */
  }
})

onUnmounted(() => {
  disposed = true
  unlistenNetTest?.()
})

async function handleSave() {
  if (useBuildTaskStore().busy || backing.value) { ElMessage.warning('请等待构建或备份恢复结束后保存设置'); return }
  if (!settings.value) return
  saving.value = true
  try {
    await useProjectStore().flushPending()
    settingsSave = backend.saveSettings(settings.value)
    storage.value = await settingsSave
    ElMessage.success('设置已保存（目录已就绪）')
  } catch (e) {
    ElMessage.error(`保存失败: ${toAppError(e).message}`)
  } finally {
    settingsSave = null
    saving.value = false
  }
}

/** 弹窗内逐目标实时测试；直接使用界面当前配置（未保存也生效） */
async function runTest() {
  if (!settings.value || testing.value) return
  const snapshot: AppSettings = JSON.parse(JSON.stringify(settings.value))

  // 预置目标（启用代理时先测代理通道；auth/registry 是拉镜像的实际依赖）
  const targets: string[] = []
  const proxyOn = snapshot.proxy?.enabled && !!snapshot.proxy.host.trim() && !!snapshot.proxy.port
  if (proxyOn) targets.push('https://www.google.com/')
  targets.push('https://auth.docker.io/token', 'https://registry-1.docker.io/v2/', 'https://docker.m.daocloud.io/v2/')

  testItems.length = 0
  for (const t of targets) {
    testItems.push({ label: labelOf(t), state: 'pending', latencyMs: 0, error: '' })
  }
  testProxyUsed.value = null
  testOpen.value = true
  testing.value = true

  try {
    const report = await backend.testNetwork(snapshot)
    testProxyUsed.value = report.proxyUsed
    // 事件丢失/乱序时以最终报告兜底刷新：仅跳过已有结论（ok/fail）的项，
    // pending/running 项必须用报告结果覆盖，否则会永久卡在"等待中"
    for (const r of report.results) {
      const item = testItems.find((t) => t.label === labelOf(r.target))
      if (item && item.state !== 'ok' && item.state !== 'fail') {
        item.state = r.ok ? 'ok' : 'fail'
        item.latencyMs = r.latencyMs
        item.error = r.ok ? '' : r.error || '不可达'
      }
    }
  } catch (e) {
    ElMessage.error(`测试失败: ${toAppError(e).message}`)
    for (const item of testItems) {
      if (item.state === 'pending' || item.state === 'running') {
        item.state = 'fail'
        item.error = '未执行'
      }
    }
  } finally {
    testing.value = false
  }
}

/** 目录选择器（Tauri 环境下弹出系统对话框；浏览器开发模式提示不可用） */
async function pickDirectory(field: 'projectsRoot' | 'imageCacheRoot' | 'dockerPkgRoot' | 'artifactRoot' | 'logRoot') {
  if (!('__TAURI_INTERNALS__' in window)) {
    ElMessage.warning('目录选择仅支持桌面应用环境（浏览器调试请手动输入路径）')
    return
  }
  try {
    const dir = await openDirectory({
      directory: true,
      multiple: false,
      title: '选择目录',
      defaultPath: storage.value![field] || undefined
    })
    if (typeof dir === 'string' && dir) {
      settings.value![field] = dir
    }
  } catch (e) {
    ElMessage.error(`打开目录选择失败: ${toAppError(e).message}`)
  }
}

// ---- 工具数据备份（方案与设置，不含镜像缓存/产物） ----
const backing = ref(false)

// ---- 目录远程更新 / 审计日志 ----
const catalogUrl = ref('')
const catalogUpdating = ref(false)
const auditOpen = ref(false)
const auditLines = ref<string[]>([])
const auditLoading = ref(false)

async function handleFetchCatalog() {
  if (!catalogUrl.value.trim()) {
    ElMessage.warning('请输入目录 JSON 的 URL')
    return
  }
  catalogUpdating.value = true
  try {
    const msg = await backend.fetchRemoteCatalog(catalogUrl.value.trim())
    window.dispatchEvent(new Event('catalog-updated'))
    ElMessage.success(msg)
  } catch (e) {
    ElMessage.error(`目录更新失败: ${toAppError(e).message}`)
  } finally {
    catalogUpdating.value = false
  }
}

async function openAudit() {
  auditOpen.value = true
  auditLoading.value = true
  try {
    auditLines.value = await backend.listAuditLog()
  } catch (e) {
    auditLines.value = [`读取失败: ${toAppError(e).message}`]
  } finally {
    auditLoading.value = false
  }
}

async function handleBackupData() {
  if (!('__TAURI_INTERNALS__' in window)) return
  try {
    const path = await saveFileDialog({
      title: '备份工具数据',
      defaultPath: `offlinepreops-backup-${new Date().toISOString().slice(0, 10)}.zip`,
      filters: [{ name: 'ZIP 备份', extensions: ['zip'] }]
    })
    if (!path) return
    backing.value = true
    try {
      await settingsSave?.catch(() => undefined)
      await useProjectStore().flushPending()
      const msg = await backend.backupAppData(path)
      ElMessage.success(msg)
    } catch (e) {
      ElMessage.error(`备份失败: ${toAppError(e).message}`)
    } finally {
      backing.value = false
    }
  } catch {
    /* 取消 */
  }
}

async function handleRestoreData() {
  if (!('__TAURI_INTERNALS__' in window)) return
  try {
    const files = await openFileDialog({
      multiple: false,
      title: '选择备份文件',
      filters: [{ name: 'ZIP 备份', extensions: ['zip'] }]
    })
    const path = Array.isArray(files) ? files[0] : files
    if (!path) return
    try {
      await ElMessageBox.confirm(
        '恢复将覆盖当前的方案与设置（镜像缓存不受影响），恢复保留本机存储路径，完成后立即重新加载；仓库、代理及中间件密码需重新填写。确定继续？',
        '恢复确认',
        { type: 'warning', confirmButtonText: '恢复', cancelButtonText: '取消' }
      )
    } catch {
      return
    }
    backing.value = true
    try {
      if (useBuildTaskStore().busy) { ElMessage.warning('请等待构建结束后恢复'); return }
      await settingsSave?.catch(() => undefined)
      await useProjectStore().close()
      const msg = await backend.restoreAppData(path)
      ElMessage.success(msg)
      // 清内存态：旧方案/设置若留在内存，随后的自动保存会反向覆盖恢复的数据
      settings.value = await backend.getSettings()
      storage.value = await backend.getStorageInfo()
      await useProjectStore().refreshSummaries()
    } catch (e) {
      ElMessage.error(`恢复失败: ${toAppError(e).message}`)
    } finally {
      backing.value = false
    }
  } catch {
    /* 取消 */
  }
}

function addMirror() {
  const v = newMirror.value.trim().replace(/^https?:\/\//, '').replace(/\/$/, '').toLowerCase()
  if (!v) return
  if (!/^[a-z0-9.-]+(:\d+)?$/i.test(v.replace(/^https?:\/\//, ''))) {
    ElMessage.warning('镜像源格式示例: docker.m.daocloud.io')
    return
  }
  if (!settings.value!.registryMirrors.includes(v)) {
    settings.value!.registryMirrors.push(v)
  }
  newMirror.value = ''
}

function removeMirror(index: number) {
  settings.value!.registryMirrors.splice(index, 1)
}
</script>

<template>
  <div class="max-w-3xl mx-auto">
    <!-- 加载失败 -->
    <div v-if="loadError" class="bg-surface-container-low rounded-xl border border-outline-variant p-8 text-center">
      <span class="material-symbols-outlined text-4xl text-error mb-3 block">cloud_off</span>
      <p class="text-sm text-on-surface-variant mb-4">读取设置失败：{{ loadError }}</p>
      <button
        class="px-5 py-1.5 rounded-lg text-xs font-bold border border-primary/50 text-primary hover:bg-primary/10 transition-colors"
        @click="loadAll"
      >
        重试
      </button>
    </div>

    <!-- 加载中 -->
    <div v-else-if="loading" class="py-20 text-center text-on-surface-variant text-sm">设置加载中…</div>

    <template v-else-if="settings && storage">
    <h1 class="font-headline text-2xl font-bold tracking-tight mb-2">设置</h1>
    <p class="text-on-surface-variant text-sm mb-6">
      大文件路径均可自定义，避免写满系统盘；留空表示使用默认值
    </p>

    <!-- 存储路径 -->
    <section class="bg-surface-container-low rounded-xl border border-outline-variant p-5 mb-6">
      <h3 class="text-sm font-headline font-bold text-primary uppercase tracking-widest mb-4">
        存储路径
      </h3>
      <div class="flex flex-col gap-5">
        <div v-for="f in ROOT_FIELDS" :key="f.key">
          <div class="flex items-center gap-2 mb-1.5">
            <span class="text-sm font-medium">{{ f.label }}</span>
            <span class="text-[10px] text-on-surface-variant/50">{{ f.desc }}</span>
          </div>
          <el-input
            v-model="(settings![f.key] as string | null)"
            :placeholder="`默认: ${storage![f.effKey]}`"
            class="font-mono"
            clearable
          >
            <template #append>
              <button
                class="flex items-center gap-1"
                title="选择目录"
                @click="pickDirectory(f.key as any)"
              >
                <span class="material-symbols-outlined text-base">folder_open</span>
                选择
              </button>
            </template>
          </el-input>
        </div>
      </div>
      <div class="text-xs text-on-surface-variant/60 mt-4">
        默认全部在软件所在目录的 data/ 下（便携式布局，整个目录拷走即迁移）；软件若安装在系统保护目录（如
        Program Files），请选择其他可写位置。注意：更改路径只影响新数据写入，
        <b>不会自动搬运</b>已有数据（请手动移动旧目录内容到新位置，镜像缓存可重新拉取）。
      </div>
    </section>

    <!-- 镜像源 -->
    <section class="bg-surface-container-low rounded-xl border border-outline-variant p-5 mb-6">
      <h3 class="text-sm font-headline font-bold text-primary uppercase tracking-widest mb-4">
        镜像源（M1 在线拉取时按序尝试）
      </h3>
      <div class="flex flex-col gap-2 mb-3">
        <div
          v-for="(m, i) in settings?.registryMirrors ?? []"
          :key="m"
          class="flex items-center gap-3 bg-surface-container rounded-lg px-3 py-2 border border-outline-variant"
        >
          <span class="font-mono text-xs">{{ i + 1 }}.</span>
          <span class="font-mono text-sm flex-1">{{ m }}</span>
          <button
            class="material-symbols-outlined text-base text-on-surface-variant hover:text-error transition-colors"
            @click="removeMirror(i)"
          >
            close
          </button>
        </div>
      </div>
      <div class="flex gap-2">
        <el-input v-model="newMirror" placeholder="添加镜像源，如 docker.m.daocloud.io" @keyup.enter="addMirror" />
        <el-button @click="addMirror">添加</el-button>
      </div>
    </section>

    <!-- 网络代理 -->
    <section class="bg-surface-container-low rounded-xl border border-outline-variant p-5 mb-6">
      <div class="flex items-center justify-between mb-1">
        <h3 class="text-sm font-headline font-bold text-primary uppercase tracking-widest">
          网络代理
        </h3>
        <el-switch v-model="settings!.proxy!.enabled" />
      </div>
      <p class="text-xs text-on-surface-variant/60 mb-4">
        访问 Docker Hub / Google 等外网（镜像拉取、版本查询、引擎下载）需要走代理；
        国内镜像源与私有仓库可直连。SOCKS5 模式下 DNS 也经代理解析。
      </p>

      <div v-if="settings!.proxy!.enabled" class="grid grid-cols-2 gap-x-6 gap-y-4">
        <div>
          <div class="text-xs text-on-surface-variant mb-1.5">代理类型</div>
          <el-select v-model="settings!.proxy!.scheme">
            <el-option value="http" label="HTTP 代理" />
            <el-option value="socks5" label="SOCKS5 代理" />
          </el-select>
        </div>
        <div>
          <div class="text-xs text-on-surface-variant mb-1.5">端口</div>
          <el-input-number
            v-model="settings!.proxy!.port"
            :min="1"
            :max="65535"
            :value-on-clear="7890"
            class="w-full"
            controls-position="right"
          />
        </div>
        <div class="col-span-2">
          <div class="text-xs text-on-surface-variant mb-1.5">代理地址</div>
          <el-input
            v-model="settings!.proxy!.host"
            placeholder="127.0.0.1（Clash/v2ray 等本地代理）"
            class="font-mono"
          />
          <div class="text-[10px] text-on-surface-variant/50 mt-1 font-mono">
            生效地址：{{ settings!.proxy!.scheme === 'socks5' ? 'socks5h' : 'http' }}://{{ settings!.proxy!.host }}:{{ settings!.proxy!.port }}
          </div>
        </div>
        <div>
          <div class="text-xs text-on-surface-variant mb-1.5">认证用户名（选填）</div>
          <el-input v-model="settings!.proxy!.username" placeholder="无则留空" />
        </div>
        <div>
          <div class="text-xs text-on-surface-variant mb-1.5">认证密码（选填）</div>
          <el-input
            v-model="settings!.proxy!.password"
            type="password"
            show-password
            autocomplete="new-password"
            placeholder="无则留空"
          />
        </div>
        <div class="col-span-2">
          <div class="text-xs text-on-surface-variant mb-1.5">直连地址（no_proxy，逗号分隔，选填）</div>
          <el-input
            v-model="settings!.proxy!.noProxy"
            placeholder="如 localhost,127.0.0.1,10.*,192.168.*,*.internal.example.cn"
            class="font-mono"
          />
          <div class="text-[10px] text-on-surface-variant/50 mt-1">
            列表中的地址不走代理（私有 Harbor、内网源填这里）
          </div>
        </div>
      </div>

      <!-- 连通性测试 -->
      <div class="mt-4 pt-4 border-t border-outline-variant">
        <div class="flex items-center gap-3 mb-2">
          <button
            class="px-4 py-1 rounded-lg text-xs font-bold border border-primary/50 text-primary hover:bg-primary/10 transition-colors"
            :disabled="testing"
            @click="runTest"
          >
            {{ testing ? '测试中…' : '测试连通性' }}
          </button>
          <span class="text-[10px] text-on-surface-variant/50">
            使用界面当前配置测试（无需先保存）；401 状态即代表源可达
          </span>
        </div>
      </div>
    </section>

    <!-- 中间件目录更新 -->
    <section class="bg-surface-container-low rounded-xl border border-outline-variant p-5 mb-6">
      <h3 class="text-sm font-headline font-bold text-primary uppercase tracking-widest mb-3">
        中间件目录
      </h3>
      <div class="flex gap-2">
        <el-input
          v-model="catalogUrl"
          placeholder="目录 JSON 地址（如公司 GitLab Pages / 静态文件 URL）"
          class="font-mono"
          @keyup.enter="handleFetchCatalog"
        />
        <button
          class="px-4 py-1 rounded-lg text-xs font-bold border border-primary/50 text-primary hover:bg-primary/10 transition-colors shrink-0"
          :disabled="catalogUpdating"
          @click="handleFetchCatalog"
        >
          {{ catalogUpdating ? '更新中…' : '检查并应用' }}
        </button>
      </div>
      <div class="text-[10px] text-on-surface-variant/50 mt-2">
        远程模板可执行容器启动和健康检查命令，请使用可信发布地址。已导出/构建的方案保留模板快照。
      </div>
    </section>

    <!-- 操作审计日志 -->
    <section class="bg-surface-container-low rounded-xl border border-outline-variant p-5 mb-6">
      <div class="flex items-center justify-between">
        <h3 class="text-sm font-headline font-bold text-primary uppercase tracking-widest">
          操作审计日志
        </h3>
        <button
          class="px-4 py-1 rounded-lg text-xs font-bold border border-outline-variant text-on-surface-variant hover:border-primary hover:text-primary transition-colors"
          @click="openAudit"
        >
          查看最近操作
        </button>
      </div>
      <div class="text-[10px] text-on-surface-variant/50 mt-2">
        构建/删除/导入导出/恢复等关键操作留痕（本地 audit.log，保留全部）
      </div>
    </section>

    <!-- 工具数据备份 -->
    <section class="bg-surface-container-low rounded-xl border border-outline-variant p-5 mb-6">
      <h3 class="text-sm font-headline font-bold text-primary uppercase tracking-widest mb-3">
        工具数据备份
      </h3>
      <div class="flex items-center gap-3 flex-wrap">
        <button
          class="px-4 py-1.5 rounded-lg text-xs font-bold border border-primary/50 text-primary hover:bg-primary/10 transition-colors"
          :disabled="backing || saving"
          @click="handleBackupData"
        >
          备份方案与设置（zip）
        </button>
        <button
          class="px-4 py-1.5 rounded-lg text-xs font-bold border border-outline-variant text-on-surface-variant hover:border-primary hover:text-primary transition-colors"
          :disabled="backing || saving"
          @click="handleRestoreData"
        >
          从备份恢复
        </button>
      </div>
      <div class="text-[10px] text-on-surface-variant/50 mt-2">
        备份含全部方案与设置（不含镜像缓存与构建产物，体积很小）；换机/重装时恢复用
      </div>
    </section>

    <!-- 生效路径总览 -->
    <section v-if="storage" class="bg-surface-container rounded-xl border border-outline-variant p-5 mb-6">
      <h3 class="text-sm font-headline font-bold text-primary uppercase tracking-widest mb-3">
        当前生效路径
      </h3>
      <div class="font-mono text-xs flex flex-col gap-1.5 text-on-surface-variant">
        <div>方案: {{ storage.projectsRoot }}</div>
        <div>缓存: {{ storage.imageCacheRoot }}</div>
        <div>安装包库: {{ storage.dockerPkgRoot }}</div>
        <div>产物: {{ storage.artifactRoot }}</div>
        <div>日志: {{ storage.logRoot }}</div>
        <div>配置: {{ storage.configDir }}</div>
      </div>
    </section>

    <div class="flex justify-end">
      <button
        class="px-8 py-2.5 rounded-xl font-headline font-bold text-xs uppercase tracking-wider transition-all bg-gradient-to-br from-primary to-primary-dim text-on-primary hover:opacity-90 active:scale-95 disabled:opacity-30"
        :disabled="saving || backing"
        @click="handleSave"
      >
        {{ saving ? '保存中…' : '保存设置' }}
      </button>
    </div>
    </template>
  </div>

  <!-- 连通性测试弹窗（逐目标实时进度） -->
  <el-dialog
    v-model="testOpen"
    title="网络连通性测试"
    width="560"
    :close-on-click-modal="!testing"
    :close-on-press-escape="!testing"
    :show-close="!testing"
  >
    <div class="mb-3 font-mono text-xs text-on-surface-variant">
      <template v-if="testing">
        正在逐目标探测（每个最长 15 秒），窗口可正常操作…
      </template>
      <template v-else>
        测试通道：
        <span :class="testProxyUsed ? 'text-success' : 'text-on-surface-variant'">
          {{ testProxyUsed ? `经代理 ${testProxyUsed}` : '直连（未启用代理）' }}
        </span>
      </template>
    </div>
    <div class="flex flex-col gap-2">
      <div
        v-for="item in testItems"
        :key="item.label"
        class="flex items-center gap-3 text-xs bg-surface-container rounded-lg px-3 py-2.5 border border-outline-variant"
      >
        <!-- 状态图标 -->
        <span
          v-if="item.state === 'running'"
          class="material-symbols-outlined text-lg text-primary animate-spin"
          >progress_activity</span
        >
        <span
          v-else-if="item.state === 'ok'"
          class="material-symbols-outlined text-lg text-success"
          >check_circle</span
        >
        <span
          v-else-if="item.state === 'fail'"
          class="material-symbols-outlined text-lg text-error"
          >cancel</span
        >
        <span v-else class="material-symbols-outlined text-lg text-on-surface-variant/30"
          >schedule</span
        >
        <span class="w-44 shrink-0">{{ item.label }}</span>
        <span
          class="flex-1 truncate"
          :class="item.state === 'ok' ? 'text-success' : item.state === 'fail' ? 'text-error' : 'text-on-surface-variant/50'"
        >
          {{
            item.state === 'pending'
              ? '等待中'
              : item.state === 'running'
                ? '探测中…'
                : item.state === 'ok'
                  ? `可达 · ${item.latencyMs}ms`
                  : item.error || '不可达'
          }}
        </span>
      </div>
    </div>
    <div class="text-[10px] text-on-surface-variant/50 mt-3">
      诊断参考：代理通道✗ → 检查代理软件/端口/类型（HTTP 端口选 HTTP，勿选 SOCKS5）；
      代理通道✓ 但 Docker 端点✗ → 代理节点访问不了 Docker，请切换节点（镜像拉取会自动回退国内源兜底）；
      daocloud✗ → 内网限制了国内镜像源，需联系网络组
    </div>
    <template #footer>
      <el-button :disabled="testing" @click="testOpen = false">关闭</el-button>
      <el-button type="primary" :disabled="testing" @click="runTest">重新测试</el-button>
    </template>
  </el-dialog>
</template>
