<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { ElMessage } from 'element-plus'
import { backend, toAppError } from '@/api/backend'
import type { AppSettings, StorageInfo, ConnectivityResult, ProxyConfig } from '@/types/project'

const loading = ref(true)
const loadError = ref('')
const saving = ref(false)
const settings = ref<AppSettings | null>(null)
const storage = ref<StorageInfo | null>(null)
const newMirror = ref('')
const testing = ref(false)
const connectivity = ref<ConnectivityResult[]>([])

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

onMounted(loadAll)

/** 保存后立即测一次连通性（读数展示在代理卡片里） */
async function handleSave() {
  if (!settings.value) return
  saving.value = true
  try {
    storage.value = await backend.saveSettings(settings.value)
    ElMessage.success('设置已保存（目录已就绪）')
    runTest()
  } catch (e) {
    ElMessage.error(`保存失败: ${toAppError(e).message}`)
  } finally {
    saving.value = false
  }
}

async function runTest() {
  testing.value = true
  try {
    connectivity.value = await backend.testNetwork()
  } catch (e) {
    ElMessage.error(`测试失败: ${toAppError(e).message}`)
  } finally {
    testing.value = false
  }
}

function targetLabel(url: string): string {
  if (url.includes('registry-1.docker.io')) return 'Docker Hub（官方源）'
  if (url.includes('daocloud')) return '国内镜像源（daocloud）'
  return url
}

function addMirror() {
  const v = newMirror.value.trim()
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
          />
        </div>
      </div>
      <div class="text-xs text-on-surface-variant/60 mt-4">
        提示：更换目录后，新数据写入新位置；已有数据的迁移功能将在后续版本提供（当前可手动移动后在此填入新路径）。
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
            保存设置后测试生效；401 状态即代表源可达（正常认证质询）
          </span>
        </div>
        <div v-if="connectivity.length" class="flex flex-col gap-1.5">
          <div
            v-for="c in connectivity"
            :key="c.target"
            class="flex items-center gap-3 text-xs font-mono bg-surface-container rounded-lg px-3 py-1.5 border border-outline-variant"
          >
            <span
              class="w-2 h-2 rounded-full shrink-0"
              :class="c.ok ? 'bg-success' : 'bg-error'"
            ></span>
            <span class="w-48 shrink-0">{{ targetLabel(c.target) }}</span>
            <span class="flex-1 truncate" :class="c.ok ? 'text-success' : 'text-error'">
              {{ c.ok ? `可达 · ${c.latencyMs}ms` : c.error || '不可达' }}
            </span>
          </div>
        </div>
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
        :disabled="saving"
        @click="handleSave"
      >
        {{ saving ? '保存中…' : '保存设置' }}
      </button>
    </div>
    </template>
  </div>
</template>
