<script setup lang="ts">
import { computed } from 'vue'
import { VueFlow, type Node, type Edge } from '@vue-flow/core'
import '@vue-flow/core/dist/style.css'
import '@vue-flow/core/dist/theme-default.css'
import { useProjectStore } from '@/stores/project'

const store = useProjectStore()

const servers = computed(() => store.project?.servers ?? [])
const rules = computed(() => store.project?.networkRules ?? [])

const nodes = computed<Node[]>(() => {
  const perRow = 3
  return servers.value.map((s, i) => {
    const instances = store.project?.instances.filter((x) => x.serverId === s.id) ?? []
    const exposed = instances
      .flatMap((x) => x.ports.filter((p) => p.expose).map((p) => `${p.host}/${p.protocol}`))
      .join('  ')
    const lines = [
      `<b>${s.name}</b>`,
      `<span style="opacity:.7;font-family:monospace;font-size:10px">${s.arch} · ${s.ip || '无IP'}</span>`,
      instances.length
        ? `<span style="font-size:10px;opacity:.8">${instances.map((x) => x.instanceName).join(' / ')}</span>`
        : `<span style="font-size:10px;opacity:.4">（无实例）</span>`,
      exposed ? `<span style="font-family:monospace;font-size:10px;opacity:.8">${exposed}</span>` : ''
    ].filter(Boolean)
    return {
      id: s.id,
      type: 'input',
      position: { x: (i % perRow) * 260, y: Math.floor(i / perRow) * 200 },
      data: { label: lines.join('<br/>') },
      style: {
        background: 'var(--t-surface-container)',
        color: 'var(--t-on-surface)',
        border: '1px solid var(--t-outline-variant)',
        borderRadius: '12px',
        padding: '10px 14px',
        width: '220px',
        fontSize: '12px',
        textAlign: 'left'
      }
    } as Node
  })
})

const edges = computed<Edge[]>(() => {
  // 同一对服务器聚合成一条边，标注端口列表
  const grouped = new Map<string, { from: string; to: string; ports: Set<number>; desc: Set<string> }>()
  for (const r of rules.value) {
    const key = `${r.fromServerId}->${r.toServerId}`
    if (!grouped.has(key)) {
      grouped.set(key, {
        from: r.fromServerId,
        to: r.toServerId,
        ports: new Set(),
        desc: new Set()
      })
    }
    const g = grouped.get(key)!
    g.ports.add(r.toPort)
    if (r.description) g.desc.add(r.description)
  }
  return Array.from(grouped.values()).map((g, i) => ({
    id: `e-${i}`,
    source: g.from,
    target: g.to,
    label: Array.from(g.ports).sort((a, b) => a - b).join(','),
    animated: true,
    labelStyle: { fill: 'var(--t-on-surface-variant)', fontSize: '10px', fontFamily: 'monospace' },
    labelBgStyle: { fill: 'var(--t-surface)', fillOpacity: 0.85 },
    style: { stroke: 'var(--t-primary)', strokeWidth: 1.5 }
  })) as Edge[]
})
</script>

<template>
  <div class="bg-surface-container-low rounded-xl border border-outline-variant p-2 h-[420px]">
    <VueFlow
      v-if="nodes.length"
      :nodes="nodes"
      :edges="edges"
      :fit-view-on-init="true"
      :nodes-draggable="true"
      :zoom-on-scroll="true"
      :pan-on-drag="true"
    >
      <template #node-input="nodeProps">
        <div
          :style="(nodeProps as any).node.style"
          class="vue-flow-node-custom"
          v-html="(nodeProps as any).node.data.label"
        />
      </template>
    </VueFlow>
    <div v-else class="h-full flex items-center justify-center text-sm text-on-surface-variant">
      暂无服务器：请先在「服务器清单」录入
    </div>
  </div>
</template>

<style scoped>
.vue-flow-node-custom {
  cursor: grab;
}
:deep(.vue-flow__handle) {
  background: var(--t-primary);
  border: none;
  width: 6px;
  height: 6px;
}
:deep(.vue-flow__edge-path) {
  stroke: var(--t-primary);
}
</style>
