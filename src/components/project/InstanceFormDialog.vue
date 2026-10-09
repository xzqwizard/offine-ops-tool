<script setup lang="ts">
import { withTag } from '@/utils/imageReference'
import { ref, computed, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { useProjectStore } from '@/stores/project'
import { backend, toAppError } from '@/api/backend'
import type { MiddlewareTemplate } from '@/types/catalog'
import type { ImageInspect } from '@/types/images'
import type { MiddlewareInstance, PortBinding, ServerInfo } from '@/types/project'
import { genId } from '@/utils/id'
import { genStrongPassword } from '@/utils/password'
import { platformMatches, supportedArch } from '@/utils/platform'

const props = defineProps<{
  modelValue: boolean
  /** null = 新增 */
  editingId: string | null
  servers: ServerInfo[]
  templates: MiddlewareTemplate[]
  submitting: boolean
}>()

const emit = defineEmits<{
  (e: 'update:modelValue', v: boolean): void
  (e: 'save', inst: MiddlewareInstance, template?: MiddlewareTemplate): void
  (e: 'saveMany', insts: MiddlewareInstance[], template?: MiddlewareTemplate): void
}>()

const store = useProjectStore()

const formServerId = ref('')
/** 新增模式的批量目标（多选） */
const formServerIds = ref<string[]>([])
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
const savedCustomTemplate = ref<MiddlewareTemplate | null>(null)
const customDataVolume = ref('')
const customDataUser = ref('')
const customCommand = ref('[]')
const readiness = ref<'none' | 'exec' | 'tcp'>('none')
const readinessCommand = ref('["curl", "-f", "http://127.0.0.1:8080/health"]')
const readinessTimeout = ref(60)
let tagsSeq = 0
let inspectSeq = 0

const categories = computed(() => {
  const set = new Set(props.templates.map((t) => t.category))
  return ['全部', ...Array.from(set)]
})

const filteredTemplates = computed(() => {
  return props.templates.filter((t) => {
    if (category.value !== '全部' && t.category !== category.value) return false
    if (
      keyword.value &&
      !`${t.displayName}${t.defaultImage}`.toLowerCase().includes(keyword.value.toLowerCase())
    )
      return false
    return true
  })
})

const selectedTemplate = computed(() =>
  savedCustomTemplate.value?.id === selectedTemplateId.value ? savedCustomTemplate.value : props.templates.find((t) => t.id === selectedTemplateId.value)
)

const selectedServer = computed(() => props.servers.find((s) => s.id === formServerId.value))

const archMismatch = computed(() => {
  if (mode.value !== 'catalog' || !selectedServer.value || !selectedTemplate.value) return false
  if (!selectedTemplate.value.supportedArches.length) return false
  return !supportedArch(selectedTemplate.value.supportedArches, selectedServer.value.arch)
})

// 在线检查结果插入：目录模式在版本 tag 之后，手动模式在镜像引用之后
const tagOptions = computed(() => {
  if (!selectedTemplate.value) return []
  if (!tagsLoaded.value) return selectedTemplate.value.recommendedTags
  // 推荐置顶 + 在线列表
  const rec = selectedTemplate.value.recommendedTags
  return [...rec, ...onlineTags.value.filter((t) => !rec.includes(t))]
})
// ---- 在线查询（tag 列表 / 架构矩阵）----
const onlineTags = ref<string[]>([])
const tagsLoading = ref(false)
const tagsLoaded = ref(false)
const inspect = ref<ImageInspect | null>(null)
const inspecting = ref(false)
/** 编辑实例时保留原 digest（避免编辑一次就丢失已锁定的 sha256） */
const editingDigest = ref('')
const editingReference = ref('')

async function loadOnlineTags() {
  if (!selectedTemplate.value) return
  const seq = ++tagsSeq
  const image = selectedTemplate.value.defaultImage
  tagsLoading.value = true
  try {
    const tags = await backend.listImageTags(image, store.project?.registry ?? undefined)
    if (seq !== tagsSeq || image !== selectedTemplate.value?.defaultImage || !props.modelValue) return
    onlineTags.value = tags
    tagsLoaded.value = true
    ElMessage.success(`已获取 ${tags.length} 个版本（可输入过滤）`)
  } catch (e) {
    if (seq === tagsSeq) ElMessage.warning(toAppError(e).message)
  } finally {
    if (seq === tagsSeq) tagsLoading.value = false
  }
}

/** 当前引用（目录模式按模板+tag，手动模式按输入） */
const currentReference = computed(() => {
  if (mode.value === 'custom') {
    return customImage.value.trim() || ''
  }
  return selectedTemplate.value ? withTag(selectedTemplate.value.defaultImage, tag.value) : ''
})

async function checkImage() {
  const seq = ++inspectSeq
  const ref = currentReference.value
  if (!ref) {
    ElMessage.warning('请先填写镜像引用')
    return
  }
  inspecting.value = true
  inspect.value = null
  inspectRef.value = ref
  try {
    const checked = await backend.inspectImage(ref, store.project?.registry ?? undefined)
    if (seq === inspectSeq && props.modelValue && inspectRef.value === ref && currentReference.value === ref) inspect.value = checked
  } catch (e) {
    if (seq === inspectSeq) ElMessage.error(`镜像检查失败: ${toAppError(e).message}`)
  } finally {
    if (seq === inspectSeq) inspecting.value = false
  }
}

/** 在线检查结果：所选服务器架构是否受支持（null=未检查或检查的不是当前引用）。
 * 匹配规则：忽略 variant 后缀——arm64 服务器可运行 linux/arm64 与 linux/arm64:v8；
 * 32 位 arm（linux/arm:v7）不与 arm64 混淆。 */
const inspectRef = ref('')
/** 单个平台条目是否匹配服务器架构。
 * 忽略 variant（arm64:v8 属于 arm64）；处理 OCI 架构名别名：
 * 镜像清单用 goarch（loong64），服务器侧用 loongarch64。 */
function archMatches(a: string): boolean {
  return !!selectedServer.value && platformMatches(a, selectedServer.value.arch)
}
const onlineArchOk = computed<boolean | null>(() => {
  if (!inspect.value || !selectedServer.value) return null
  // 检查结果必须对应当前引用（改了 tag/镜像后旧结论作废）
  if (inspectRef.value !== currentReference.value) return null
  return inspect.value.arches.some(archMatches)
})

const savingTpl = ref(false)

function customTemplate(displayName: string, id: string, category: string): MiddlewareTemplate {
  const parseArgs = (text: string) => {
    const args: unknown = JSON.parse(text)
    if (!Array.isArray(args) || !args.every(a => typeof a === 'string')) throw new Error('命令必须是 JSON 字符串数组')
    return args as string[]
  }
  const ref = customImage.value.trim()
  const base = ref.split('@')[0]
  const ci = base.lastIndexOf(':')
  const name = ci > base.lastIndexOf('/') ? base.slice(0, ci) : base
  if (readiness.value === 'tcp' && !ports.value.some(p => p.expose && p.protocol === 'tcp')) throw new Error('TCP 探针需要至少一个发布的 TCP 端口')
  return {
    id, displayName, category,
    defaultImage: ref.includes('@') ? name + ref.slice(ref.indexOf('@')) : name,
    recommendedTags: [extractTag(ref)].filter(t => t && t !== 'latest'),
    supportedArches: [],
    ports: ports.value.map(p => ({ name: p.name, container: p.container, defaultHost: p.host, protocol: p.protocol })),
    envHints: Object.keys(params.value).map(k => ({ key: k, label: k, secret: /PASSWORD|SECRET|TOKEN|_KEY$|_PASS$|_PWD$/i.test(k), required: false, default: '' })),
    dataVolume: customDataVolume.value.trim(), dataUser: customDataUser.value.trim() || null,
    command: parseArgs(customCommand.value), dependsOn: [],
    healthCheck: readiness.value === 'none' ? null : { type: readiness.value, cmd: readiness.value === 'exec' ? parseArgs(readinessCommand.value) : [] },
    healthTimeoutSec: readinessTimeout.value, minMemoryGb: 0.5, kernelReqs: [], remark: `自定义：${ref}`
  }
}

/** 手动输入的镜像存为"我的中间件"（下次目录直接选） */
async function saveAsCustom() {
  if (savingTpl.value) return
  savingTpl.value = true
  if (!customImage.value.trim()) {
    ElMessage.warning('请先填写镜像引用')
    savingTpl.value = false
    return
  }
  const displayName = instanceName.value.trim() || customImage.value.split('/').pop() || '自定义'
  try {
    await ElMessageBox.prompt('保存名称（目录中显示）', '存为我的中间件', {
      inputValue: displayName,
      confirmButtonText: '保存',
      cancelButtonText: '取消'
    }).then(async ({ value }) => {
      if (!value.trim()) return
      savingTpl.value = true
      const tpl = customTemplate(value.trim(), genId('custom'), '我的中间件')
      await backend.saveCustomTemplate(tpl, '我的中间件')
      savedCustomTemplate.value = tpl
      selectedTemplateId.value = tpl.id
      tag.value = extractTag(customImage.value.trim()) || 'latest'
      mode.value = 'catalog'
      window.dispatchEvent(new Event('catalog-updated'))
      ElMessage.success(`已保存「${value.trim()}」，下次可从目录直接选择`)
    })
  } catch (e) {
    // 区分用户取消与真实错误：取消是 string 'cancel'
    if (e !== 'cancel' && e !== 'close') {
      ElMessage.error(`保存失败: ${toAppError(e).message}`)
    }
  } finally {
    savingTpl.value = false
  }
}

/** 切到手动输入时自动带入当前引用，方便在架构不匹配时直接改镜像地址 */
watch(mode, (m) => {
  if (m === 'custom' && !customImage.value.trim()) {
    const t = selectedTemplate.value
    customImage.value = t ? withTag(t.defaultImage, tag.value) : ''
    if (t) {
      customDataVolume.value = t.dataVolume
      customDataUser.value = t.dataUser ?? ''
      customCommand.value = JSON.stringify(t.command)
      readiness.value = t.healthCheck?.type === 'tcp' ? 'tcp' : t.healthCheck ? 'exec' : 'none'
      readinessCommand.value = JSON.stringify(t.healthCheck?.cmd ?? [])
      readinessTimeout.value = t.healthTimeoutSec ?? 60
    }
  }
})

watch(
  () => props.modelValue,
  (open) => {
    if (open) init()
  }
)

// 多选集合变化时同步首台到 formServerId（selectedServer/在线检查依赖它）
watch(formServerIds, (ids) => {
  formServerId.value = ids[0] ?? ''
})

function init() {
  ++tagsSeq
  ++inspectSeq
  savedCustomTemplate.value = null
  customDataVolume.value = ''
  customDataUser.value = ''
  customCommand.value = '[]'
  readiness.value = 'none'
  readinessTimeout.value = 60
  keyword.value = ''
  category.value = '全部'
  // 在线检查/在线 tag 状态必须随弹窗重置：残留的旧结论会用旧镜像的
  // 架构检查结果阻断本次保存，旧 tag 列表会混入当前模板的版本下拉
  inspect.value = null
  inspecting.value = false
  onlineTags.value = []
  tagsLoaded.value = false
  tagsLoading.value = false
  const editing = props.editingId
    ? store.project?.instances.find((i) => i.id === props.editingId)
    : null
  if (editing) {
    const config = props.templates.find(t => t.id === editing.templateId && t.category === '实例配置')
    formServerId.value = editing.serverId
    formServerIds.value = [editing.serverId]
    mode.value = editing.templateId === 'custom' || config ? 'custom' : 'catalog'
    selectedTemplateId.value = editing.templateId
    customImage.value = mode.value === 'custom' ? editing.image : ''
    if (config) {
      customDataVolume.value = config.dataVolume
      customDataUser.value = config.dataUser ?? ''
      customCommand.value = JSON.stringify(config.command)
      readiness.value = config.healthCheck?.type === 'tcp' ? 'tcp' : config.healthCheck ? 'exec' : 'none'
      readinessCommand.value = JSON.stringify(config.healthCheck?.cmd ?? [])
      readinessTimeout.value = config.healthTimeoutSec ?? 60
    } else if (editing.templateId === 'custom') {
      // Preserve the legacy raw custom mount until the user explicitly changes it.
      customDataVolume.value = '/data'
    }
    tag.value = editing.templateId === 'custom' ? '' : extractTag(editing.image)
    instanceName.value = editing.instanceName
    ports.value = JSON.parse(JSON.stringify(editing.ports))
    params.value = Object.fromEntries(
      Object.entries(editing.params).map(([k, v]) => [k, String(v ?? '')])
    )
    localImageTar.value = editing.localImageTar
    editingDigest.value = editing.digest
    editingReference.value = editing.image
  } else {
    formServerId.value = props.servers[0]?.id ?? ''
    formServerIds.value = props.servers[0]?.id ? [props.servers[0].id] : []
    mode.value = 'catalog'
    selectedTemplateId.value = ''
    tag.value = ''
    instanceName.value = ''
    ports.value = []
    params.value = {}
    localImageTar.value = ''
    customImage.value = ''
    editingDigest.value = ''
  }
}

function extractTag(image: string): string {
  // 取最后一个 ":" 之后（digest 优先取 @ 前的部分）
  const ref = image.split('@')[0]
  const idx = ref.lastIndexOf(':')
  return idx > ref.lastIndexOf('/') ? ref.slice(idx + 1) : ''
}

function selectTemplate(t: MiddlewareTemplate) {
  ++tagsSeq
  onlineTags.value = []
  tagsLoaded.value = false
  tagsLoading.value = false
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
  params.value = Object.fromEntries(
    t.envHints.filter((h) => h.default).map((h) => [h.key, h.default])
  )
}

function addPort() {
  ports.value.push({
    name: `port-${ports.value.length + 1}`,
    host: 8080,
    container: 8080,
    protocol: 'tcp',
    expose: true
  })
}

function removePort(index: number) {
  ports.value.splice(index, 1)
}

function handleSave() {
  if (props.submitting || savingTpl.value) return
  // 编辑=单服务器（formServerId 由 init 赋值）；新增统一读多选集合，
  // 避免读到只在 init 时赋值的过期单选值（选 1 台会部署到错误机器）
  const targetIds = props.editingId
    ? [formServerId.value]
    : formServerIds.value.filter(Boolean)
  if (!targetIds.length || targetIds.some((id) => !id)) {
    ElMessage.warning('请选择目标服务器')
    return
  }
  if (mode.value === 'catalog' && !selectedTemplate.value) {
    ElMessage.warning('请先选择中间件')
    return
  }
  if (targetIds.length === 1 && selectedServer.value && archMismatch.value) {
    ElMessage.error(
      `架构不兼容：${selectedTemplate.value!.displayName} 不支持 ${selectedServer.value.arch}，请更换镜像/版本或改用手动输入`
    )
    return
  }
  if (!localImageTar.value.trim() && targetIds.length === 1 && selectedServer.value && onlineArchOk.value === false) {
    ElMessage.error(
      `在线检查确认：${currentReference.value} 不支持 linux/${selectedServer.value.arch}`
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
  for (const h of mode.value === 'catalog' ? selectedTemplate.value?.envHints ?? [] : []) {
    if (h.required && !params.value[h.key]?.trim()) {
      ElMessage.warning(`参数「${h.label}」为必填项`)
      return
    }
  }
  for (const p of ports.value) {
    if ((p.expose && (p.host < 1 || p.host > 65535)) || p.container < 1 || p.container > 65535) {
      ElMessage.warning(`端口超出范围 (1-65535)：${p.host}→${p.container}`)
      return
    }
  }

  let image =
    mode.value === 'custom'
      ? customImage.value.trim()
      : withTag(selectedTemplate.value!.defaultImage, tag.value.trim())
  if (localImageTar.value.trim() && image.includes('@')) {
    ElMessage.warning('本地 tar 使用独立内容校验；请将镜像引用改为该归档中的 repo:tag，移除 @digest')
    return
  }

  const cleanParams: Record<string, unknown> = {}
  for (const [k, v] of Object.entries(params.value)) {
    if (v !== null && v !== undefined && String(v).trim() !== '') cleanParams[k] = v
  }

  // 批量模式：静态目录/在线检查不支持的架构跳过（构建校验兜底，这里给明确提示）
  const skipped: string[] = []
  const finalIds =
    props.editingId || targetIds.length === 1
      ? targetIds
      : targetIds.filter((id) => {
          const s = props.servers.find((x) => x.id === id)
          if (!s) return false
          const mismatch = (mode.value === 'catalog' && selectedTemplate.value && !supportedArch(selectedTemplate.value.supportedArches, s.arch)) ||
            (!localImageTar.value.trim() && inspect.value && inspectRef.value === image && !inspect.value.arches.some(a => platformMatches(a, s.arch)))
          if (mismatch) skipped.push(`${s.name}(${s.arch})`)
          return !mismatch
        })
  if (!finalIds.length) {
    ElMessage.error(`所选服务器的架构均不支持该中间件：${skipped.join('、')}`)
    return
  }
  if (skipped.length) {
    ElMessage.warning(`已跳过架构不兼容的服务器：${skipped.join('、')}`)
  }

  const digest = localImageTar.value.trim() ? '' :
    inspect.value && inspectRef.value === image && inspect.value.digest
      ? inspect.value.digest
      : image === editingReference.value ? editingDigest.value : ''

  let template: MiddlewareTemplate | undefined
  if (mode.value === 'custom') {
    try { template = customTemplate(instanceName.value.trim(), selectedTemplateId.value.startsWith('instcfg-') ? selectedTemplateId.value : genId('instcfg'), '实例配置') }
    catch (e) { ElMessage.warning(toAppError(e).message); return }
  }
  const instances: MiddlewareInstance[] = finalIds.map((serverId) => ({
    id: props.editingId ?? genId('inst'),
    serverId,
    templateId: template?.id ?? (mode.value === 'custom' ? 'custom' : selectedTemplate.value!.id),
    image,
    digest,
    instanceName: instanceName.value.trim(),
    params: cleanParams,
    ports: JSON.parse(JSON.stringify(ports.value)),
    localImageTar: localImageTar.value.trim()
  }))

  if (instances.length === 1 && (props.editingId || formServerIds.value.length <= 1)) {
    emit('save', instances[0], template)
  } else {
    emit('saveMany', instances, template)
  }
}
</script>

<template>
  <el-dialog
    :model-value="modelValue"
    :title="editingId ? '编辑中间件实例' : '添加中间件'"
    width="920"
    top="4vh"
    :close-on-click-modal="!submitting && !savingTpl"
    :close-on-press-escape="!submitting && !savingTpl"
    :show-close="!submitting && !savingTpl"
    @update:model-value="emit('update:modelValue', $event)"
  >
    <!-- 目标服务器（新增可多选批量部署，编辑单台） -->
    <div class="flex items-center gap-3 mb-4">
      <span class="text-sm text-on-surface-variant shrink-0">部署到服务器：</span>
      <el-select
        v-if="editingId"
        v-model="formServerId"
        placeholder="选择服务器"
        class="w-72"
      >
        <el-option
          v-for="s in servers"
          :key="s.id"
          :value="s.id"
          :label="`${s.name}（${s.arch} / IP ${s.ip || '未填'}）`"
        />
      </el-select>
      <el-select
        v-else
        v-model="formServerIds"
        multiple
        collapse-tags
        collapse-tags-tooltip
        placeholder="选择服务器（可多选批量部署）"
        class="flex-1 max-w-xl"
      >
        <el-option
          v-for="s in servers"
          :key="s.id"
          :value="s.id"
          :label="`${s.name}（${s.arch} / IP ${s.ip || '未填'}）`"
        />
      </el-select>
      <span v-if="!editingId && formServerIds.length > 1" class="text-[10px] text-on-surface-variant/50 shrink-0">
        批量模式：参数共享，架构不兼容的服务器自动跳过
      </span>
    </div>

    <el-radio-group v-model="mode" class="mb-3">
      <el-radio-button value="catalog">从目录选择</el-radio-button>
      <el-radio-button value="custom">手动输入镜像</el-radio-button>
    </el-radio-group>

    <!-- 镜像在线检查（通用） -->
    <div class="mb-3 bg-surface-container rounded-lg border border-outline-variant px-3 py-2 flex items-center gap-3 flex-wrap">
      <span class="font-mono text-xs text-on-surface-variant truncate flex-1 min-w-40">
        {{ currentReference || '（未填写镜像引用）' }}
      </span>
      <button
        type="button"
        class="px-3 py-1 rounded-lg text-xs font-bold border border-primary/50 text-primary hover:bg-primary/10 transition-colors shrink-0"
        :disabled="inspecting || !currentReference"
        @click="checkImage"
      >
        {{ inspecting ? '检查中…' : '在线检查镜像' }}
      </button>
      <template v-if="inspect">
        <el-tag
          v-for="a in inspect.arches"
          :key="a"
          size="small"
          class="font-mono"
          :type="selectedServer && archMatches(a) ? 'success' : 'info'"
        >
          {{ a }}{{ selectedServer && archMatches(a) ? ' ✓' : '' }}
        </el-tag>
        <span
          v-if="onlineArchOk === false"
          class="text-xs text-error font-bold shrink-0"
        >
          ⚠ 不支持当前服务器架构（{{ selectedServer?.arch }}）
        </span>
      </template>
    </div>

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
          <div
            v-if="!filteredTemplates.length"
            class="text-xs text-on-surface-variant py-6 text-center"
          >
            无匹配目录项
          </div>
        </div>
      </div>

      <!-- 目录右侧表单 -->
      <div v-if="selectedTemplate" class="overflow-y-auto max-h-[480px] pr-1">
        <el-alert v-if="archMismatch" type="error" :closable="false" class="mb-4"
          >镜像 {{ selectedTemplate.defaultImage }} 预计不支持
          {{ selectedServer?.arch }} 架构（以 M1 在线 manifest 校验为准），请谨慎选择或改用手动输入</el-alert
        >
        <el-form label-width="110px" label-position="left" @submit.prevent>
          <el-form-item label="版本 tag">
            <el-select
              v-model="tag"
              filterable
              allow-create
              default-first-option
              :loading="tagsLoading"
              class="w-full"
            >
              <el-option
                v-for="t in tagOptions"
                :key="t"
                :value="t"
                :label="tagsLoaded && !selectedTemplate!.recommendedTags.includes(t) ? t : t"
              />
            </el-select>
            <div class="flex items-center gap-2 mt-1">
              <button
                type="button"
                class="text-xs text-primary hover:underline"
                :disabled="tagsLoading"
                @click="loadOnlineTags"
              >
                {{ tagsLoading ? '获取中…' : tagsLoaded ? '刷新在线版本列表' : '获取在线版本列表' }}
              </button>
              <span class="text-[10px] text-on-surface-variant/50">默认仅展示推荐版本</span>
            </div>
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
                  <el-input-number
                    v-model="row.host"
                    :min="1"
                    :max="65535"
                    size="small"
                    controls-position="right"
                    class="w-28"
                  />
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
            >
              <template #append>
                <button
                  type="button"
                  title="自动生成强密码（16位，含大小写/数字/符号）"
                  @click="params[h.key] = genStrongPassword()"
                >
                  <span class="material-symbols-outlined text-base">casino</span>
                  生成
                </button>
              </template>
            </el-input>
            <el-input v-else v-model="params[h.key]" :placeholder="h.default || '选填'" />
            <div v-if="h.required" class="text-xs text-error mt-0.5">必填</div>
          </el-form-item>
          <el-form-item label="本地镜像 tar">
            <el-input
              v-model="localImageTar"
              placeholder="M0：docker save 导出的 tar 文件绝对路径（选填）"
            />
            <div class="text-xs text-on-surface-variant mt-1">
              填写后按本地 tar 内容校验并清除仓库 digest 锁；未填写时使用可信缓存或自动拉取。
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
          <div class="flex items-center gap-2 mt-1">
            <span class="text-xs text-on-surface-variant">支持完整语法 registry/repo:tag（可含 @sha256: 摘要）</span>
            <button
              type="button"
              class="text-xs text-primary hover:underline"
              :disabled="savingTpl"
              @click="saveAsCustom"
            >
              ★ 存为我的中间件
            </button>
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
                <el-input-number
                  v-model="row.host"
                  :min="1"
                  :max="65535"
                  size="small"
                  controls-position="right"
                  class="w-28"
                />
              </template>
            </el-table-column>
            <el-table-column label="容器端口" width="130">
              <template #default="{ row }">
                <el-input-number
                  v-model="row.container"
                  :min="1"
                  :max="65535"
                  size="small"
                  controls-position="right"
                  class="w-28"
                />
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
        <el-divider content-position="left">部署配置（随实例保存，可另存为我的中间件）</el-divider>
        <el-form-item label="持久化目录"><el-input v-model="customDataVolume" placeholder="容器内绝对路径，如 /var/lib/app；留空不挂载" /></el-form-item>
        <el-form-item label="运行属主"><el-input v-model="customDataUser" placeholder="UID:GID，如 1000:1000；留空使用镜像默认" /></el-form-item>
        <el-form-item label="启动命令"><el-input v-model="customCommand" placeholder='JSON 数组，如 ["app", "--serve"]' /></el-form-item>
        <el-form-item label="就绪检查">
          <el-select v-model="readiness"><el-option value="none" label="仅检查稳定运行" /><el-option value="tcp" label="TCP：检查首个发布 TCP 端口" /><el-option value="exec" label="执行业务探针（可检查 HTTP）" /></el-select>
          <el-input v-if="readiness === 'exec'" v-model="readinessCommand" type="textarea" class="mt-2" placeholder='镜像内命令 JSON 数组，如 ["curl", "-f", "http://127.0.0.1:8080/health"]' />
          <p class="text-xs text-on-surface-variant mt-1">探针需在镜像内可执行并反映业务就绪；在线镜像检查只确认镜像信息。</p>
        </el-form-item>
        <el-form-item label="就绪超时"><el-input-number v-model="readinessTimeout" :min="1" :max="3600" /> 秒</el-form-item>
      </el-form>
    </div>

    <template #footer>
      <el-button :disabled="submitting || savingTpl" @click="emit('update:modelValue', false)">取消</el-button>
      <el-button type="primary" :loading="submitting" :disabled="savingTpl" @click="handleSave">确定并保存</el-button>
    </template>
  </el-dialog>
</template>
