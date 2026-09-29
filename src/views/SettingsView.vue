<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { ElMessage } from 'element-plus'
import { backend, toAppError } from '@/api/backend'
import type { AppSettings, StorageInfo } from '@/types/project'

const loading = ref(true)
const saving = ref(false)
const settings = ref<AppSettings | null>(null)
const storage = ref<StorageInfo | null>(null)
const newMirror = ref('')

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

onMounted(async () => {
  try {
    ;[settings.value, storage.value] = await Promise.all([
      backend.getSettings(),
      backend.getStorageInfo()
    ])
  } catch (e) {
    ElMessage.error(`读取设置失败: ${toAppError(e).message}`)
  } finally {
    loading.value = false
  }
})

async function handleSave() {
  if (!settings.value) return
  saving.value = true
  try {
    storage.value = await backend.saveSettings(settings.value)
    ElMessage.success('设置已保存（目录已就绪）')
  } catch (e) {
    ElMessage.error(`保存失败: ${toAppError(e).message}`)
  } finally {
    saving.value = false
  }
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
  <div class="max-w-3xl mx-auto" v-loading="loading">
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
  </div>
</template>
