<script setup lang="ts">
import { computed, ref } from 'vue'
import { useVueFlow, type NodeProps } from '@vue-flow/core'
import { lt } from '@/i18n'
import { statusColor } from '@/lib/findings'
import { t } from '@/i18n'
import { useApp } from '@/stores/app'
import { useEditor } from '@/stores/editor'
import { DEFAULT_ZONE_H, DEFAULT_ZONE_W } from './layout'

const props = defineProps<NodeProps>()
const app = useApp()
const editor = useEditor()
const { viewport } = useVueFlow()

const id = computed(() => props.data.id as string)
const zone = computed(() => editor.zoneById.get(id.value))
const def = computed(() => (zone.value ? app.index?.zoneTypes.get(zone.value.type) : undefined))
const selected = computed(() => editor.selection?.type === 'zone' && editor.selection.id === id.value)
const status = computed(() => editor.zoneStatus.get(id.value))
const added = computed(() => !!editor.previewSets?.added.zones.has(id.value))
const ghost = computed(() => !!editor.previewSets?.removed.zones.has(id.value))
const lit = computed(() => !!editor.highlight?.zones.includes(id.value))
const simDown = computed(() => !!editor.sim?.result?.failed.some((f) => f === zone.value?.name))

const live = ref<{ w: number; h: number } | null>(null)
const w = computed(() => live.value?.w ?? zone.value?.ui?.w ?? DEFAULT_ZONE_W)
const h = computed(() => live.value?.h ?? zone.value?.ui?.h ?? DEFAULT_ZONE_H)

const tags = computed(() => {
  const z = zone.value
  if (!z) return []
  const out: { text: string; cls: string }[] = []
  const role = z.role
  const tier = z.props.tier ? `Tier ${z.props.tier}` : ''
  if (role) out.push({ text: [role.toUpperCase(), tier].filter(Boolean).join(' · '), cls: 'role-' + role })
  else if (tier) out.push({ text: tier, cls: 'role-none' })
  return out
})

function onClick(e: MouseEvent) {
  e.stopPropagation()
  editor.select({ type: 'zone', id: id.value })
}

function startResize(e: PointerEvent) {
  if (editor.readonly) return
  e.stopPropagation()
  e.preventDefault()
  const el = e.currentTarget as HTMLElement
  el.setPointerCapture(e.pointerId)
  const sx = e.clientX
  const sy = e.clientY
  const w0 = w.value
  const h0 = h.value
  const move = (ev: PointerEvent) => {
    const z = viewport.value.zoom || 1
    live.value = { w: Math.max(240, w0 + (ev.clientX - sx) / z), h: Math.max(160, h0 + (ev.clientY - sy) / z) }
  }
  const up = () => {
    el.removeEventListener('pointermove', move)
    el.removeEventListener('pointerup', up)
    if (live.value) editor.resizeZone(id.value, live.value.w, live.value.h)
    live.value = null
  }
  el.addEventListener('pointermove', move)
  el.addEventListener('pointerup', up)
}
</script>

<template>
  <div v-if="zone" class="arch-zone" :class="{ selected, added, ghost, lit, down: simDown }" :style="{ width: w + 'px', height: h + 'px', '--zc': def?.color ?? 'var(--line-strong)' }">
    <div class="zone-head" :title="def ? lt(def.name) : zone.type" @click="onClick">
      <span class="zname truncate">{{ zone.name }}</span>
      <span v-for="g in tags" :key="g.text" class="ztag" :class="g.cls">{{ g.text }}</span>
      <span v-if="status" class="zdot" :style="{ background: statusColor(status) }" />
      <span v-if="simDown" class="zfail">{{ t('node.state.down') }}</span>
    </div>
    <div v-if="!editor.readonly" class="zone-resize nodrag nopan" @pointerdown="startResize" />
  </div>
</template>

<style scoped>
.arch-zone {
  position: relative; border: 1px solid var(--line-strong); border-radius: 14px; background: rgba(255, 255, 255, .55); pointer-events: none;
}
.arch-zone.selected { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
.arch-zone.added { border: 1.5px dashed var(--info-dot); }
.arch-zone.ghost { opacity: .5; border: 1.5px dashed var(--crit); }
.arch-zone.lit { border-color: var(--accent); box-shadow: 0 0 0 4px var(--accent-soft); }
.arch-zone.down { background: rgba(253, 237, 238, .7); border-color: var(--crit); }
.zone-head { position: absolute; left: 18px; top: 14px; max-width: calc(100% - 36px); display: flex; align-items: center; gap: 9px; pointer-events: auto; cursor: grab; padding: 2px 4px; margin: -2px -4px; border-radius: 6px; }
.zone-head:hover { background: rgba(20, 23, 31, .04); }
.zname { font-weight: 650; font-size: 14px; letter-spacing: -0.005em; color: var(--text); }
.ztag { font-family: var(--mono); font-size: 10.5px; letter-spacing: .05em; padding: 2px 8px; border-radius: 5px; white-space: nowrap; }
.role-active { color: var(--ok); background: var(--ok-bg); }
.role-standby { color: var(--accent); background: var(--accent-soft); }
.role-witness { color: var(--edge-quorum); background: #f0e9fb; }
.role-none { color: var(--muted); background: var(--surface-sunken); }
.zdot { width: 9px; height: 9px; border-radius: 50%; flex: none; }
.zfail { font-size: 10.5px; font-weight: 700; color: #fff; background: var(--crit); border-radius: 9px; padding: 1px 8px; }
.zone-resize { position: absolute; right: 2px; bottom: 2px; width: 18px; height: 18px; cursor: nwse-resize; pointer-events: auto; opacity: 0; transition: opacity var(--dur-fast); background: linear-gradient(135deg, transparent 50%, var(--faint) 50%, var(--faint) 58%, transparent 58%, transparent 72%, var(--faint) 72%, var(--faint) 80%, transparent 80%); border-bottom-right-radius: 12px; }
.arch-zone:hover .zone-resize, .arch-zone.selected .zone-resize { opacity: .8; }
</style>
