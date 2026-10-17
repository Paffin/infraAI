<script setup lang="ts">
import { computed, ref } from 'vue'
import { useVueFlow, type NodeProps } from '@vue-flow/core'
import { lt, t } from '@/i18n'
import { statusColor, statusLabel } from '@/lib/findings'
import { iconFor } from '@/lib/icons'
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

const icon = computed(() => iconFor(def.value?.icon))
/** How many cards stand in the zone, those of the zones inside it included. */
const count = computed(() => {
  const inside = new Set([id.value])
  for (let grew = true; grew; ) {
    grew = false
    for (const z of editor.shown.zones) if (z.parentId && inside.has(z.parentId) && !inside.has(z.id)) { inside.add(z.id); grew = true }
  }
  return editor.shown.nodes.filter((n) => n.zoneId && inside.has(n.zoneId)).length
})

/** The role of the zone (active, standby, witness) and its tier, each a pill of its own. */
const tags = computed(() => {
  const z = zone.value
  if (!z) return []
  const out: { text: string; cls: string }[] = []
  if (z.role) out.push({ text: z.role, cls: 'role-' + z.role })
  if (z.props.tier) out.push({ text: `Tier ${z.props.tier}`, cls: 'tier' })
  return out
})

/**
 * What a screen reader and a voice control call the head of the zone: the words written on it in the order they are written (the name, the pills, how many cards
 * stand in it, that it has failed), then after a comma what is not written: the type of the zone and the finding its dot stands for.
 */
const label = computed(() => {
  const z = zone.value
  if (!z) return undefined
  const written = [z.name, ...tags.value.map((g) => g.text), count.value ? t('projects.nodes', { n: count.value }) : '', simDown.value ? t('node.state.down') : ''].filter(Boolean).join(' ')
  const extras = [
    def.value ? lt(def.value.name) : z.type,
    status.value ? statusLabel(status.value).toLowerCase() : '',
    added.value ? t('inspector.proposed.short') : '',
    ghost.value ? t('card.removed') : '',
  ].filter(Boolean)
  return `${written}, ${extras.join(', ')}`
})

// the keys of the head are the canvas's (Canvas.vue): Enter selects the zone
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
    <div class="zone-head" :title="def ? lt(def.name) : zone.type" role="button" tabindex="-1" data-kbd="zone" :data-id="id" :aria-pressed="selected" :aria-label="label" @click="onClick">
      <span class="zico" aria-hidden="true"><component :is="icon" :size="14" :stroke-width="2" /></span>
      <span class="zname truncate">{{ zone.name }}</span>
      <!-- the spaces keep the words of a reader apart; the pills, the count and the dot are in the label of the head too -->
      <template v-for="g in tags" :key="g.text">{{ ' ' }}<span class="ztag" :class="g.cls">{{ g.text }}</span></template>
      {{ ' ' }}<span v-if="count" class="zmeta">{{ t('projects.nodes', { n: count }) }}</span>
      <span v-if="status" class="zdot" :style="{ background: statusColor(status) }" />
      {{ ' ' }}<span v-if="simDown" class="zfail">{{ t('node.state.down') }}</span>
    </div>
    <div v-if="!editor.readonly" class="zone-resize nodrag nopan" @pointerdown="startResize" />
  </div>
</template>

<style scoped>
.arch-zone {
  position: relative; border: 1px solid color-mix(in srgb, var(--zc) 28%, var(--line-strong)); border-radius: 16px; pointer-events: none;
  background: linear-gradient(180deg, color-mix(in srgb, var(--zc) 6%, rgba(255, 255, 255, .72)), color-mix(in srgb, var(--zc) 2%, rgba(255, 255, 255, .5)));
  box-shadow: 0 1px 2px rgba(20, 23, 31, .03); transition: border-color var(--dur-base), background-color var(--dur-base), box-shadow var(--dur-fast);
}
.arch-zone.selected { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
.arch-zone.added { border: 1.5px dashed var(--info-dot); }
.arch-zone.ghost { opacity: .5; border: 1.5px dashed var(--crit); }
.arch-zone.lit { border-color: var(--accent); box-shadow: 0 0 0 4px var(--accent-soft); }
/* the zone that has failed: red, hatched */
.arch-zone.down { border-color: var(--crit); background: repeating-linear-gradient(135deg, rgba(192, 38, 45, .045) 0 11px, rgba(192, 38, 45, .09) 11px 22px), rgba(253, 237, 238, .78); }
.zone-head { position: absolute; left: 14px; top: 12px; max-width: calc(100% - 28px); display: flex; align-items: center; gap: 8px; pointer-events: auto; cursor: grab; padding: 3px 8px 3px 4px; margin: -3px -8px -3px -4px; border-radius: 999px; transition: background-color var(--dur-fast); }
.zone-head:hover { background: rgba(255, 255, 255, .7); }
.zico { width: 24px; height: 24px; border-radius: 8px; flex: none; display: inline-flex; align-items: center; justify-content: center; color: color-mix(in srgb, var(--zc) 80%, #000); background: color-mix(in srgb, var(--zc) 13%, #fff); box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--zc) 26%, #fff); }
.zname { font-weight: 650; font-size: 14px; letter-spacing: -0.005em; color: var(--text); }
.ztag { font-size: 10px; font-weight: 650; letter-spacing: .07em; text-transform: uppercase; padding: 2px 8px; border-radius: 999px; white-space: nowrap; line-height: 1.3; }
.ztag.tier { font-family: var(--mono); font-weight: 500; letter-spacing: .03em; text-transform: none; font-size: 10.5px; color: var(--muted); background: var(--surface-sunken); }
.role-active { color: var(--ok); background: var(--ok-bg); }
.role-standby { color: var(--accent); background: var(--accent-soft); }
.role-witness { color: var(--edge-quorum); background: #f0e9fb; }
.zmeta { font-size: 11.5px; color: var(--muted); white-space: nowrap; font-variant-numeric: tabular-nums; }
.zdot { width: 9px; height: 9px; border-radius: 50%; flex: none; }
.zfail { font-size: 10.5px; font-weight: 700; color: #fff; background: var(--crit); border-radius: 9px; padding: 1px 8px; }
.zone-resize { position: absolute; right: 2px; bottom: 2px; width: 18px; height: 18px; cursor: nwse-resize; pointer-events: auto; opacity: 0; transition: opacity var(--dur-fast); background: linear-gradient(135deg, transparent 50%, var(--faint) 50%, var(--faint) 58%, transparent 58%, transparent 72%, var(--faint) 72%, var(--faint) 80%, transparent 80%); border-bottom-right-radius: 12px; }
.arch-zone:hover .zone-resize, .arch-zone.selected .zone-resize { opacity: .8; }
</style>
