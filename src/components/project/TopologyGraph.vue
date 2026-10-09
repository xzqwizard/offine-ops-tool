<script setup lang="ts">
import { computed } from 'vue'
import { VueFlow, Handle, Position, type Node, type Edge, type NodeProps } from '@vue-flow/core'
import '@vue-flow/core/dist/style.css'
import '@vue-flow/core/dist/theme-default.css'
import { useProjectStore } from '@/stores/project'

const store = useProjectStore()

interface ServerNodeData {
  name: string
  arch: string
  ip: string
  instances: string
  ports: string
}

const servers = computed(() => store.project?.servers ?? [])
const rules = computed(() => store.project?.networkRules ?? [])

const nodes = computed<Node<ServerNodeData>[]>(() => {
  const perRow = 3
  return servers.value.map((s, i) => {
    const instances = store.project?.instances.filter((x) => x.serverId === s.id) ?? []
    const ports = instances
      .flatMap((x) => x.ports.filter((p) => p.expose).map((p) => `${p.host}/${p.protocol}`))
      .join('  ')
    return {
      id: s.id,
      type: 'server',
      position: { x: (i % perRow) * 260, y: Math.floor(i / perRow) * 210 },
      data: {
        name: s.name,
        arch: s.arch,
        ip: s.ip || '无IP',
        instances: instances.length
          ? instances.map((x) => x.instanceName).join(' / ')
          : '（无实例）',
        ports
      }
    } as Node<ServerNodeData>
  })
})

const edges = computed<Edge[]>(() => {
  // 同一对服务器聚合成一条边；自指规则（from==to）无拓扑意义，跳过
  const grouped = new Map<string, { from: string; to: string; ports: Set<string> }>()
  for (const r of rules.value) {
    if (r.fromServerId === r.toServerId) continue
    const key = `${r.fromServerId}->${r.toServerId}`
    if (!grouped.has(key)) {
      grouped.set(key, { from: r.fromServerId, to: r.toServerId, ports: new Set() })
    }
    grouped.get(key)!.ports.add(`${r.toPort}/${r.protocol}`)
  }
  return Array.from(grouped.values()).map((g, i) => ({
    id: `e-${i}`,
    source: g.from,
    target: g.to,
    label: Array.from(g.ports).sort((a, b) => parseInt(a) - parseInt(b) || a.localeCompare(b)).join(', '),
    animated: true,
    labelStyle: { fill: 'var(--t-on-surface-variant)', fontSize: '10px', fontFamily: 'monospace' },
    labelBgStyle: { fill: 'var(--t-surface)', fillOpacity: 0.85 },
    style: { stroke: 'var(--t-primary)', strokeWidth: 1.5 }
  })) as Edge[]
})

function nodeStyle(p: NodeProps<ServerNodeData>) {
  return {
    background: 'var(--t-surface-container)',
    color: 'var(--t-on-surface)',
    border: '1px solid var(--t-outline-variant)',
    borderRadius: '12px',
    padding: '10px 14px',
    width: '220px',
    fontSize: '12px',
    textAlign: 'left' as const
  }
}
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
      <template #node-server="props">
        <div :style="nodeStyle(props)" class="server-node">
          <Handle type="target" :position="Position.Left" />
          <Handle type="source" :position="Position.Right" />
          <div class="font-bold">{{ props.data.name }}</div>
          <div class="mono dim">{{ props.data.arch }} · {{ props.data.ip }}</div>
          <div class="small">{{ props.data.instances }}</div>
          <div v-if="props.data.ports" class="mono dim">{{ props.data.ports }}</div>
        </div>
      </template>
    </VueFlow>
    <div v-else class="h-full flex items-center justify-center text-sm text-on-surface-variant">
      暂无服务器：请先在「服务器清单」录入
    </div>
  </div>
</template>

<style scoped>
.server-node {
  cursor: grab;
}
.server-node .mono {
  font-family: 'Fira Code', Consolas, monospace;
}
.server-node .dim {
  opacity: 0.7;
}
.server-node .small {
  font-size: 10px;
}
:deep(.vue-flow__handle) {
  background: var(--t-primary);
  border: none;
  width: 6px;
  height: 6px;
}
</style>
