<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { ElMessage } from 'element-plus'
import { useProjectStore } from '@/stores/project'
import type { MiddlewareTemplate } from '@/types/catalog'
import type { MiddlewareInstance, PortBinding, ServerInfo } from '@/types/project'
import { genId } from '@/utils/id'

const props = defineProps<{
  modelValue: boolean
  /** null = 新增 */
  editingId: string | null
  server: ServerInfo | null
  templates: MiddlewareTemplate[]
}>()

const emit = defineEmits<{
  (e: 'update:modelValue', v: boolean): void
  (e: 'save', inst: MiddlewareInstance): void
}>()

const store = useProjectStore()

const mode = ref<'catalog' | 'custom'>('catalog')
const selectedTemplateId = ref('')
const tag = ref('')
const instanceName = ref('')
const ports = ref<PortBinding[]>([])
const params = ref<Record<string, string>>({})
const localImageTar = ref('')
const customImage = ref('')
const category = ref('全部')
const keyword = ref('')

const categories = computed(() => {
  const set = new Set(props.templates.map((t) => t.category))
  return ['全部', ...Array.from(set)]
})

const filteredTemplates = computed(() => {
  return props.templates.filter((t) => {
    if (category.value !== '全部' && t.category !== category.value) return false
    if (keyword.value && !`${t.displayName}${t.defaultImage}`.toLowerCase().includes(keyword.value.toLowerCase()))
      return false
    return true
  })
})

const selectedTemplate = computed(() =>
  props.templates.find((t) => t.id === selectedTemplateId.value)
)

const archMismatch = computed(() => {
  if (!props.server || !selectedTemplate.value) return false
  if (!selectedTemplate.value.supportedArches.length) return false
  return !selectedTemplate.value.supportedArches.includes(props.server.arch)
})

watch(
  () => props.modelValue,
  (open) => {
    if (open) init()
  }
)

function init() {
  keyword.value = ''
  category.value = '全部'
  const editing = props.editingId
    ? store.project?.instances.find((i) => i.id === props.editingId)
    : null
  if (editing) {
    mode.value = editing.templateId === 'custom' ? 'custom' : 'catalog'
    selectedTemplateId.value = editing.templateId
    customImage.value = editing.templateId === 'custom' ? editing.image : ''
    tag.value = editing.templateId === 'custom' ? '' : extractTag(editing.image)
    instanceName.value = editing.instanceName
    ports.value = JSON.parse(JSON.stringify(editing.ports))
    params.value = Object.fromEntries(
      Object.entries(editing.params).map(([k, v]) => [k, String(v ?? '')])
    )
    localImageTar.value = editing.localImageTar
  } else {
    mode.value = 'catalog'
    selectedTemplateId.value = ''
    tag.value = ''
    instanceName.value = ''
    ports.value = []
    params.value = {}
    localImageTar.value = ''
    customImage.value = ''
  }
}

function extractTag(image: string): string {
  // 取最后一个 ":" 之后（digest 优先取 @ 前的部分）
  const ref = image.split('@')[0]
  const idx = ref.lastIndexOf(':')
  return idx > ref.lastIndexOf('/') ? ref.slice(idx + 1) : ''
}

function selectTemplate(t: MiddlewareTemplate) {
  selectedTemplateId.value = t.id
  tag.value = t.recommendedTags[0] ?? ''
  if (!instanceName.value || !props.editingId) {
    instanceName.value = t.id
  }
  ports.value = t.ports.map((p) => ({
    name: p.name,
    host: p.defaultHost,
    container: p.container,
    protocol: p.protocol,
    expose: true
  }))
  params.value = Object.fromEntries(t.envHints.filter((h) => h.default).map((h) => [h.key, h.default]))
}

function addPort() {
  ports.value.push({ name: `port-${ports.value.length + 1}`, host: 8080, container: 8080, protocol: 'tcp', expose: true })
}

function removePort(index: number) {
  ports.value.splice(index, 1)
}

function handleSave() {
  if (!props.server) return
  if (mode.value === 'catalog' && !selectedTemplate.value) {
    ElMessage.warning('请先选择中间件')
    return
  }
  if (archMismatch.value) {
    ElMessage.error(
      `架构不兼容：${selectedTemplate.value!.displayName} 不支持 ${props.server.arch}，请更换镜像/版本或使用手动输入`
    )
    return
  }
  if (!instanceName.value.trim()) {
    ElMessage.warning('实例名不能为空')
    return
  }
  if (mode.value === 'custom' && !customImage.value.trim()) {
    ElMessage.warning('镜像引用不能为空')
    return
  }
  if (mode.value === 'catalog' && !tag.value.trim()) {
    ElMessage.warning('版本 tag 不能为空')
    return
  }
  for (const h of selectedTemplate.value?.envHints ?? []) {
    if (h.required && !params.value[h.key]?.trim()) {
      ElMessage.warning(`参数「${h.label}」为必填项`)
      return
    }
  }
  for (const p of ports.value) {
    if (p.host < 1 || p.host > 65535 || p.container < 1 || p.container > 65535) {
      ElMessage.warning(`端口超出范围 (1-65535)：${p.host}→${p.container}`)
      return
    }
  }

  const image =
    mode.value === 'custom'
      ? customImage.value.trim()
      : `${selectedTemplate.value!.defaultImage}:${tag.value.trim()}`

  const cleanParams: Record<string, unknown> = {}
  for (const [k, v] of Object.entries(params.value)) {
    if (v !== null && v !== undefined && String(v).trim() !== '') cleanParams[k] = v
  }

  const inst: MiddlewareInstance = {
    id: props.editingId ?? genId('inst'),
    serverId: props.server.id,
    templateId: mode.value === 'custom' ? 'custom' : selectedTemplate.value!.id,
    image,
    digest: '',
    instanceName: instanceName.value.trim(),
    params: cleanParams,
    ports: JSON.parse(JSON.stringify(ports.value)),
    localImageTar: localImageTar.value.trim()
  }
  emit('save', inst)
}
</script>

<template>
  <el-dialog
    :model-value="modelValue"
    :title="editingId ? '编辑中间件实例' : '添加中间件'"
    width="920"
    top="4vh"
    @update:model-value="emit('update:modelValue', $event)"
  >
    <el-radio-group v-model="mode" class="mb-4" :disabled="!!editingId">
      <el-radio-button value="catalog">从目录选择</el-radio-button>
      <el-radio-button value="custom">手动输入镜像</el-radio-button>
    </el-radio-group>

    <!-- 目录模式 -->
    <div v-if="mode === 'catalog'" class="grid grid-cols-[280px_1fr] gap-5">
      <div class="flex flex-col gap-2">
        <div class="flex gap-2">
          <el-select v-model="category" size="small" class="flex-1">
            <el-option v-for="c in categories" :key="c" :value="c" :label="c" />
          </el-select>
          <el-input v-model="keyword" size="small" placeholder="搜索" clearable class="w-24" />
        </div>
        <div class="overflow-y-auto max-h-[420px] flex flex-col gap-1.5 pr-1">
          <button
            v-for="t in filteredTemplates"
            :key="t.id"
            class="text-left px-3 py-2.5 rounded-lg border transition-colors"
            :class="[
              selectedTemplateId === t.id
                ? 'border-primary bg-surface-container-high'
                : 'border-outline-variant bg-surface-container-low hover:border-primary/50'
            ]"
            @click="selectTemplate(t)"
          >
            <div class="flex items-center justify-between gap-2">
              <span class="text-sm font-medium">{{ t.displayName }}</span>
              <span class="text-[10px] text-on-surface-variant/60 font-mono shrink-0">{{
                t.category
              }}</span>
            </div>
            <div class="text-[10px] font-mono text-on-surface-variant/50 truncate mt-0.5">
              {{ t.defaultImage }}
            </div>
          </button>
          <div v-if="!filteredTemplates.length" class="text-xs text-on-surface-variant py-6 text-center">
            无匹配目录项
          </div>
        </div>
      </div>

      <!-- 目录右侧表单 -->
      <div v-if="selectedTemplate" class="overflow-y-auto max-h-[480px] pr-1">
        <el-alert v-if="archMismatch" type="error" :closable="false" class="mb-4"
          >镜像 {{ selectedTemplate.defaultImage }} 预计不支持 {{ server?.arch }} 架构（以 M1 在线
          manifest 校验为准），请谨慎选择或改用手动输入</el-alert
        >
        <el-form label-width="110px" label-position="left" @submit.prevent>
          <el-form-item label="版本 tag">
            <el-select v-model="tag" filterable allow-create default-first-option class="w-full">
              <el-option v-for="t in selectedTemplate.recommendedTags" :key="t" :value="t" :label="t" />
            </el-select>
            <div class="text-xs text-on-surface-variant mt-1 font-mono">
              {{ selectedTemplate.defaultImage }}:{{ tag || '?' }}
            </div>
          </el-form-item>
          <el-form-item label="实例名">
            <el-input v-model="instanceName" placeholder="compose 服务名，如 mysql" maxlength="40" />
          </el-form-item>
          <el-form-item label="端口绑定">
            <el-table :data="ports" size="small" class="w-full">
              <el-table-column prop="name" label="名称" width="80" />
              <el-table-column label="宿主端口" width="120">
                <template #default="{ row }">
                  <el-input-number v-model="row.host" :min="1" :max="65535" size="small" controls-position="right" class="w-28" />
                </template>
              </el-table-column>
              <el-table-column label="容器端口" width="90">
                <template #default="{ row }">
                  <span class="font-mono text-xs">{{ row.container }}</span>
                </template>
              </el-table-column>
              <el-table-column label="协议" width="90">
                <template #default="{ row }">
                  <el-select v-model="row.protocol" size="small">
                    <el-option value="tcp" label="tcp" />
                    <el-option value="udp" label="udp" />
                  </el-select>
                </template>
              </el-table-column>
              <el-table-column label="对外暴露">
                <template #default="{ row }">
                  <el-switch v-model="row.expose" size="small" />
                </template>
              </el-table-column>
            </el-table>
          </el-form-item>
          <el-form-item v-for="h in selectedTemplate.envHints" :key="h.key" :label="h.label">
            <el-input
              v-if="h.secret"
              v-model="params[h.key]"
              type="password"
              show-password
              autocomplete="new-password"
            />
            <el-input v-else v-model="params[h.key]" :placeholder="h.default || '选填'" />
            <div v-if="h.required" class="text-xs text-error mt-0.5">必填</div>
          </el-form-item>
          <el-form-item label="本地镜像 tar">
            <el-input v-model="localImageTar" placeholder="M0：docker save 导出的 tar 文件绝对路径（选填）" />
            <div class="text-xs text-on-surface-variant mt-1">
              未填写时构建产物将不包含该镜像，部署前需自行 docker load
            </div>
          </el-form-item>
        </el-form>
      </div>
      <div v-else class="flex items-center justify-center text-on-surface-variant text-sm">
        ← 从左侧目录选择中间件
      </div>
    </div>

    <!-- 手动输入模式 -->
    <div v-else class="overflow-y-auto max-h-[480px]">
      <el-form label-width="110px" label-position="left" @submit.prevent>
        <el-form-item label="镜像引用" required>
          <el-input
            v-model="customImage"
            placeholder="如：registry.example.cn/gov/app:2.3.1 或 nginx:1.25.3"
            class="font-mono"
          />
          <div class="text-xs text-on-surface-variant mt-1">
            支持完整语法 registry/repo:tag（可含 @sha256: 摘要）
          </div>
        </el-form-item>
        <el-form-item label="实例名" required>
          <el-input v-model="instanceName" placeholder="compose 服务名" maxlength="40" />
        </el-form-item>
        <el-form-item label="端口绑定">
          <el-table :data="ports" size="small" class="w-full mb-2">
            <el-table-column label="名称" width="110">
              <template #default="{ row }">
                <el-input v-model="row.name" size="small" />
              </template>
            </el-table-column>
            <el-table-column label="宿主端口" width="130">
              <template #default="{ row }">
                <el-input-number v-model="row.host" :min="1" :max="65535" size="small" controls-position="right" class="w-28" />
              </template>
            </el-table-column>
            <el-table-column label="容器端口" width="130">
              <template #default="{ row }">
                <el-input-number v-model="row.container" :min="1" :max="65535" size="small" controls-position="right" class="w-28" />
              </template>
            </el-table-column>
            <el-table-column label="协议" width="90">
              <template #default="{ row }">
                <el-select v-model="row.protocol" size="small">
                  <el-option value="tcp" label="tcp" />
                  <el-option value="udp" label="udp" />
                </el-select>
              </template>
            </el-table-column>
            <el-table-column label="操作" width="70" align="center">
              <template #default="{ $index }">
                <el-button link type="danger" size="small" @click="removePort($index)">删</el-button>
              </template>
            </el-table-column>
          </el-table>
          <el-button size="small" @click="addPort">+ 添加端口</el-button>
        </el-form-item>
        <el-form-item label="本地镜像 tar">
          <el-input v-model="localImageTar" placeholder="docker save 导出的 tar 文件绝对路径（选填）" />
        </el-form-item>
      </el-form>
    </div>

    <template #footer>
      <el-button @click="emit('update:modelValue', false)">取消</el-button>
      <el-button type="primary" @click="handleSave">确定</el-button>
    </template>
  </el-dialog>
</template>
