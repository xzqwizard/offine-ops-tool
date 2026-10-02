<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useProjectStore } from '@/stores/project'
import { backend, toAppError } from '@/api/backend'
import { hasBlockingErrors, type ValidationIssue } from '@/utils/validate'
import type { MiddlewareTemplate } from '@/types/catalog'
import { ElMessage } from 'element-plus'

const store = useProjectStore()
const templates = ref<MiddlewareTemplate[]>([])
const issues = ref<ValidationIssue[] | null>(null)
const checkedRevision = ref(-1)
const checking = ref(false)

onMounted(async () => {
  try {
    templates.value = (await backend.listCatalog()).templates
  } catch (e) {
    ElMessage.error(`读取目录失败: ${toAppError(e).message}`)
  }
})

async function runCheck() {
  if (!store.project || checking.value) return
  const revision = store.revision
  const id = store.project.id
  checking.value = true
  try {
    const checked = await backend.validateProject(JSON.parse(JSON.stringify(store.project)))
    if (store.project?.id === id) { issues.value = checked; checkedRevision.value = revision }
  } catch (e) { ElMessage.error(`校验失败: ${toAppError(e).message}`) }
  finally { checking.value = false }
}

// 任何修改（防抖/commit 保存均会自增 revision）后提示结果过期
const stale = computed(() => issues.value !== null && checkedRevision.value >= 0 && checkedRevision.value !== store.revision)

const errors = computed(() => issues.value?.filter((i) => i.level === 'error') ?? [])
const warnings = computed(() => issues.value?.filter((i) => i.level === 'warning') ?? [])
const infos = computed(() => issues.value?.filter((i) => i.level === 'info') ?? [])

const levelIcon: Record<string, { icon: string; cls: string }> = {
  error: { icon: 'cancel', cls: 'text-error' },
  warning: { icon: 'warning', cls: 'text-warning' },
  info: { icon: 'info', cls: 'text-on-surface-variant' }
}
</script>

<template>
  <div>
    <div class="flex justify-between items-center mb-4">
      <p class="text-on-surface-variant text-sm">构建前的强制门禁：错误必须全部解决才能构建离线包</p>
      <button
        class="px-5 py-1.5 rounded-xl font-headline font-bold text-xs uppercase tracking-wider transition-all bg-gradient-to-br from-primary to-primary-dim text-on-primary hover:opacity-90 active:scale-95"
        :disabled="checking"
        @click="runCheck"
      >
        立即校验
      </button>
    </div>

    <div v-if="stale" class="mb-4">
      <el-alert type="warning" :closable="false" show-icon>
        方案内容已修改，校验结果可能过期，请重新执行校验
      </el-alert>
    </div>

    <div v-if="issues === null" class="py-14 text-center text-on-surface-variant text-sm">
      点击「立即校验」检查方案配置
    </div>

    <div v-else class="flex flex-col gap-4">
      <div class="grid grid-cols-3 gap-3">
        <div class="bg-surface-container rounded-xl border border-outline-variant p-4 text-center">
          <div class="text-2xl font-headline font-bold" :class="errors.length ? 'text-error' : 'text-success'">{{ errors.length }}</div>
          <div class="text-xs text-on-surface-variant mt-1">错误（阻断构建）</div>
        </div>
        <div class="bg-surface-container rounded-xl border border-outline-variant p-4 text-center">
          <div class="text-2xl font-headline font-bold" :class="warnings.length ? 'text-warning' : 'text-on-surface-variant'">
            {{ warnings.length }}
          </div>
          <div class="text-xs text-on-surface-variant mt-1">警告</div>
        </div>
        <div class="bg-surface-container rounded-xl border border-outline-variant p-4 text-center">
          <div class="text-2xl font-headline font-bold text-on-surface-variant">{{ infos.length }}</div>
          <div class="text-xs text-on-surface-variant mt-1">提示</div>
        </div>
      </div>

      <div
        v-if="!hasBlockingErrors(issues) && issues.length"
        class="bg-success/10 border border-success/30 rounded-xl p-3 text-sm text-success flex items-center gap-2"
      >
        <span class="material-symbols-outlined text-lg">check_circle</span>
        校验通过：无阻断性错误，可进入「构建」页签
      </div>

      <div class="bg-surface-container-low rounded-xl border border-outline-variant divide-y divide-outline-variant">
        <div v-for="(issue, idx) in issues" :key="idx" class="flex items-start gap-3 px-4 py-2.5">
          <span
            class="material-symbols-outlined text-lg shrink-0 mt-0.5"
            :class="levelIcon[issue.level].cls"
            >{{ levelIcon[issue.level].icon }}</span
          >
          <div class="min-w-0">
            <div class="text-sm">{{ issue.message }}</div>
            <div class="text-[10px] font-mono text-on-surface-variant/50 mt-0.5">{{ issue.code }}</div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
