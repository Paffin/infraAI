<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { ChevronDown, ChevronUp, Crosshair, GripVertical, Grid3x3, Plus, X, Zap } from 'lucide-vue-next'
import type { Scenario } from '@/api/types'
import { t } from '@/i18n'
import { resultChips, sameScenario, scenarioLabel, shortZone } from '@/lib/sim'
import { useFloat } from '@/lib/useFloat'
import { useApp } from '@/stores/app'
import { useEditor } from '@/stores/editor'
import { useUi } from '@/stores/ui'

const app = useApp()
const editor = useEditor()
const ui = useUi()
type Menu = 'sites' | 'racks' | 'link' | 'isolate' | 'multi' | 'geo'
const menu = ref<Menu | null>(null)
const q = ref('')
/** What is ticked in the "several at once" list: sites and racks, nodes, channels (`a|b`). */
const pickedZones = ref<string[]>([])
const pickedNodes = ref<string[]>([])
const pickedLinks = ref<string[]>([])
const bar = ref<HTMLElement | null>(null)
/** The panel is dragged anywhere over the canvas by what is not a control (its label, its padding, the grip); Home or a double click on the grip put it back. Folded it is a pill with the outcome. */
const fl = useFloat({ key: 'ia_whatif_pos', area: () => bar.value?.parentElement ?? null, el: () => bar.value })
function barDown(e: PointerEvent) {
  if ((e.target as HTMLElement).closest('button:not(.grip), input, label, .menu')) return
  fl.onPointerDown(e)
}

/** Up to this many sites get a button of their own; more live in a searchable list. */
const INLINE_SITES = 4

const sites = computed(() => editor.siteZones)
const racks = computed(() => editor.rackZones)
const scenarios = computed(() => editor.report?.scenarios ?? [])
// only the channels the engine found connected, not every pair of sites
const links = computed(() => scenarios.value.filter((s) => s.scenario.type === 'link').map((s) => s.scenario))
const isolates = computed(() => scenarios.value.filter((s) => s.scenario.type === 'isolate').map((s) => s.scenario))
const geos = computed(() => [...new Set(editor.arch.zones.map((z) => z.props.geo).filter((g): g is string => typeof g === 'string' && !!g))])
const active = computed(() => editor.sim?.scenario ?? null)
const res = computed(() => editor.sim?.result)
const chips = computed(() => (res.value ? resultChips(res.value) : []))
const stateTone = computed(() => (res.value?.state === 'up' ? 'ok' : res.value?.state === 'degraded' ? 'risk' : 'crit'))
const label = (sc: Scenario) => scenarioLabel(sc, editor.arch, editor.report?.units)

const filtered = <T,>(items: T[], text: (x: T) => string) => {
  const needle = q.value.trim().toLowerCase()
  return needle ? items.filter((x) => text(x).toLowerCase().includes(needle)) : items
}
const siteItems = computed(() => filtered(sites.value, (z) => z.name))
const rackItems = computed(() => filtered(racks.value, (z) => z.name))
const linkItems = computed(() => filtered(links.value, label))
const isolateItems = computed(() => filtered(isolates.value, label))
const showSearch = computed(() => {
  const n = menu.value === 'sites' ? sites.value.length : menu.value === 'racks' ? racks.value.length : menu.value === 'link' ? links.value.length : menu.value === 'isolate' ? isolates.value.length : 0
  return n > 8
})

/** A menu opens over its button, to the right; one that would run past the edge of the canvas opens to the left. */
const flip = ref(false)
const MENU_ROOM = 360
function toggle(m: Menu, e?: MouseEvent) {
  menu.value = menu.value === m ? null : m
  q.value = ''
  const btn = e?.currentTarget as HTMLElement | undefined
  const area = bar.value?.parentElement
  if (btn && area) flip.value = btn.getBoundingClientRect().left + MENU_ROOM > area.getBoundingClientRect().right
  if (m === 'multi' && menu.value) seed(active.value)
}

// ---- several failures at once: everything ticked fails together (the engine's `sites` for sites alone, `combo` for a mix)
const linkKey = (sc: Scenario) => (sc.type === 'link' ? `${sc.a}|${sc.b}` : '')
/** The picker starts from the failure on screen, so that one more can be added to it: "and if this fails too?" */
function seed(sc: Scenario | null) {
  const zones: string[] = []
  const nodes: string[] = []
  const chans: string[] = []
  const walk = (x: Scenario) => {
    if (x.type === 'zone') zones.push(x.zoneId)
    else if (x.type === 'sites') zones.push(...x.zoneIds)
    else if (x.type === 'node' || x.type === 'instance') nodes.push(x.nodeId)
    else if (x.type === 'link') chans.push(linkKey(x))
    else if (x.type === 'combo') x.items.forEach(walk)
  }
  if (sc) walk(sc)
  const known = new Set(zoneChoices.value.map((z) => z.id))
  pickedZones.value = [...new Set(zones)].filter((z) => known.has(z))
  pickedNodes.value = [...new Set(nodes)].filter((n) => editor.nodeById.has(n))
  pickedLinks.value = [...new Set(chans)].filter((k) => links.value.some((l) => linkKey(l) === k))
}
const zoneChoices = computed(() => [...sites.value, ...racks.value])
const pickedCount = computed(() => pickedZones.value.length + pickedNodes.value.length + pickedLinks.value.length)
const composed = computed<Scenario | null>(() => {
  const items: Scenario[] = [
    ...pickedZones.value.map((zoneId) => ({ type: 'zone' as const, zoneId })),
    ...pickedNodes.value.map((nodeId) => ({ type: 'node' as const, nodeId })),
    ...links.value.filter((sc) => pickedLinks.value.includes(linkKey(sc))),
  ]
  if (!items.length) return null
  if (items.length === 1) return items[0]
  return items.every((x) => x.type === 'zone') ? { type: 'sites', zoneIds: pickedZones.value } : { type: 'combo', items }
})
const nodeItems = computed(() => filtered(editor.arch.nodes, (n) => n.name))
const zoneItems = computed(() => filtered(zoneChoices.value, (z) => z.name))
/** Rows shown at a time (`editor.pickerShown`); a search finds the rest. */
const shownNodes = computed(() => nodeItems.value.slice(0, app.editorCfg.pickerShown))
const zoneOf = (id: string | null) => (id ? shortZone(editor.zoneById.get(id)?.name ?? '') : '')
/** What is selected on the diagram can be added in one click. */
const selectedOnDiagram = computed(() => {
  const nodes = editor.multi.length ? editor.multi : editor.selection?.type === 'node' ? [editor.selection.id] : []
  const zones = editor.selection?.type === 'zone' && zoneChoices.value.some((z) => z.id === editor.selection?.id) ? [editor.selection.id] : []
  return { nodes: nodes.filter((n) => editor.nodeById.has(n)), zones }
})
const canAddSelected = computed(() => selectedOnDiagram.value.nodes.length + selectedOnDiagram.value.zones.length > 0)
function addSelected() {
  pickedNodes.value = [...new Set([...pickedNodes.value, ...selectedOnDiagram.value.nodes])]
  pickedZones.value = [...new Set([...pickedZones.value, ...selectedOnDiagram.value.zones])]
}
function clearPicked() {
  pickedZones.value = []
  pickedNodes.value = []
  pickedLinks.value = []
}
function run(sc: Scenario) {
  menu.value = null
  void editor.runScenario(sc)
}
function runPicked() {
  if (composed.value) run(composed.value)
}
function pickNode() {
  menu.value = null
  const s = editor.selection
  if (s?.type === 'node') return run({ type: 'node', nodeId: s.id })
  editor.pickingNode = !editor.pickingNode
}
const isNodeActive = computed(() => active.value?.type === 'node' || active.value?.type === 'instance')
const isZoneActive = (id: string) => sameScenario(active.value, { type: 'zone', zoneId: id })

function outside(e: MouseEvent) {
  if (menu.value && bar.value && !bar.value.contains(e.target as Node)) menu.value = null
}
// capture phase: the canvas stops the mouse events of its own area, so a click on the diagram would never close a menu that waits for it to bubble
onMounted(() => document.addEventListener('mousedown', outside, true))
onBeforeUnmount(() => document.removeEventListener('mousedown', outside, true))
</script>

<template>
  <div ref="bar" class="whatif" :class="{ folded: ui.whatIfFolded, floating: fl.floating.value, dragging: fl.dragging.value, down: !fl.lowerHalf.value }" :style="fl.style.value"
       :data-overlay="fl.floating.value ? 'float' : 'bottom'" data-tour="whatif" data-testid="whatif" role="toolbar" :aria-label="t('sim.whatif')"
       @keydown.esc="menu = null" @pointerdown="barDown">
    <button class="grip" :class="{ on: fl.dragging.value }" :title="t('sim.bar.move.hint')" :aria-label="t('sim.bar.move')" data-testid="whatif-grip" @keydown="fl.onKeyDown" @dblclick="fl.reset()"><GripVertical :size="14" /></button>
    <button v-if="ui.whatIfFolded" class="lbl-btn" :aria-expanded="false" :title="t('sim.bar.unfold')" data-testid="whatif-unfold" @click="ui.whatIfFolded = false"><Zap :size="13" />{{ t('sim.whatif') }}</button>
    <span v-else class="lbl">{{ t('sim.whatif') }}</span>
    <button v-if="!ui.whatIfFolded" class="w" :class="{ on: isNodeActive, pick: editor.pickingNode }" @click="pickNode"><Crosshair :size="13" />{{ t('sim.node') }}</button>

    <template v-if="ui.whatIfFolded" />
    <template v-else-if="sites.length <= INLINE_SITES">
      <button v-for="z in sites" :key="z.id" class="w" :class="{ on: isZoneActive(z.id) }" @click="run({ type: 'zone', zoneId: z.id })">{{ shortZone(z.name) }}</button>
    </template>
    <div v-else class="pop">
      <button class="w" :class="{ on: sites.some((z) => isZoneActive(z.id)) }" @click="toggle('sites', $event)">{{ t('sim.sites') }} · {{ sites.length }}<ChevronUp :size="12" class="chev" /></button>
      <div v-if="menu === 'sites'" class="menu" :class="{ flip }">
        <input v-if="showSearch" v-model="q" class="find" :placeholder="t('sim.sites.search')" autofocus />
        <div class="items"><button v-for="z in siteItems" :key="z.id" :class="{ cur: isZoneActive(z.id) }" @click="run({ type: 'zone', zoneId: z.id })">{{ z.name }}</button></div>
      </div>
    </div>

    <div v-if="!ui.whatIfFolded && racks.length" class="pop">
      <button class="w" :class="{ on: racks.some((z) => isZoneActive(z.id)) }" @click="toggle('racks', $event)">{{ t('sim.rack') }} · {{ racks.length }}<ChevronUp :size="12" class="chev" /></button>
      <div v-if="menu === 'racks'" class="menu" :class="{ flip }">
        <input v-if="showSearch" v-model="q" class="find" :placeholder="t('sim.sites.search')" />
        <div class="items"><button v-for="z in rackItems" :key="z.id" :class="{ cur: isZoneActive(z.id) }" @click="run({ type: 'zone', zoneId: z.id })">{{ z.name }}</button></div>
      </div>
    </div>

    <div v-if="!ui.whatIfFolded && links.length" class="pop">
      <button class="w" :class="{ on: active?.type === 'link' }" @click="toggle('link', $event)">{{ t('sim.channel') }}<ChevronUp :size="12" class="chev" /></button>
      <div v-if="menu === 'link'" class="menu" :class="{ flip }">
        <input v-if="showSearch" v-model="q" class="find" :placeholder="t('sim.sites.search')" />
        <div class="items"><button v-for="sc in linkItems" :key="JSON.stringify(sc)" :class="{ cur: sameScenario(active, sc) }" @click="run(sc)">{{ label(sc) }}</button></div>
      </div>
    </div>

    <div v-if="!ui.whatIfFolded && isolates.length" class="pop">
      <button class="w" :class="{ on: active?.type === 'isolate' }" :title="t('sim.isolate.hint')" @click="toggle('isolate', $event)">{{ t('sim.isolate') }}<ChevronUp :size="12" class="chev" /></button>
      <div v-if="menu === 'isolate'" class="menu" :class="{ flip }">
        <input v-if="showSearch" v-model="q" class="find" :placeholder="t('sim.sites.search')" />
        <div class="items"><button v-for="sc in isolateItems" :key="JSON.stringify(sc)" :class="{ cur: sameScenario(active, sc) }" @click="run(sc)">{{ label(sc) }}</button></div>
      </div>
    </div>

    <div v-if="!ui.whatIfFolded && (sites.length >= 3 || (zoneChoices.length >= 2 && editor.arch.nodes.length >= 2))" class="pop">
      <button class="w" :class="{ on: active?.type === 'sites' || active?.type === 'combo' }" data-testid="whatif-compose" @click="toggle('multi', $event)">{{ t('sim.compose') }}<ChevronUp :size="12" class="chev" /></button>
      <div v-if="menu === 'multi'" class="menu wide compose" :class="{ flip }">
        <p class="mh">{{ t('sim.compose.hint') }}</p>
        <input v-model="q" class="find" :placeholder="t('sim.compose.search')" :aria-label="t('sim.compose.search')" />
        <button v-if="canAddSelected" class="addsel" data-testid="compose-selected" @click="addSelected"><Plus :size="13" />{{ t('sim.compose.selection') }}</button>
        <div class="items">
          <!-- a group of choices is a group, not a heading of the page: a screen reader reads its name once and then the boxes -->
          <div v-if="zoneItems.length" class="grp" role="group" aria-labelledby="wi-g-sites">
            <span id="wi-g-sites" class="gh">{{ t('sim.compose.sites') }}</span>
            <label v-for="z in zoneItems" :key="z.id" class="chk"><input v-model="pickedZones" type="checkbox" :value="z.id" />{{ z.name }}</label>
          </div>
          <div v-if="nodeItems.length" class="grp" role="group" aria-labelledby="wi-g-nodes">
            <span id="wi-g-nodes" class="gh">{{ t('sim.compose.nodes') }}</span>
            <label v-for="n in shownNodes" :key="n.id" class="chk"><input v-model="pickedNodes" type="checkbox" :value="n.id" />{{ n.name }}<small v-if="zoneOf(n.zoneId)">{{ zoneOf(n.zoneId) }}</small></label>
            <p v-if="nodeItems.length > shownNodes.length" class="more">{{ t('sim.compose.more', { n: nodeItems.length - shownNodes.length }) }}</p>
          </div>
          <div v-if="linkItems.length" class="grp" role="group" aria-labelledby="wi-g-links">
            <span id="wi-g-links" class="gh">{{ t('sim.compose.links') }}</span>
            <label v-for="sc in linkItems" :key="linkKey(sc)" class="chk"><input v-model="pickedLinks" type="checkbox" :value="linkKey(sc)" />{{ label(sc) }}</label>
          </div>
        </div>
        <div class="foot">
          <button class="go" :disabled="!composed" data-testid="compose-run" @click="runPicked">{{ t('sim.compose.run') }}<template v-if="pickedCount"> · {{ pickedCount }}</template></button>
          <button v-if="pickedCount" class="clear" @click="clearPicked">{{ t('sim.compose.clear') }}</button>
        </div>
      </div>
    </div>

    <div v-if="!ui.whatIfFolded && geos.length" class="pop">
      <button class="w" :class="{ on: active?.type === 'geo' }" @click="toggle('geo', $event)">{{ t('sim.domain') }}<ChevronUp :size="12" class="chev" /></button>
      <div v-if="menu === 'geo'" class="menu" :class="{ flip }"><div class="items"><button v-for="g in geos" :key="g" @click="run({ type: 'geo', geo: g })">{{ g }}</button></div></div>
    </div>
    <span v-if="!ui.whatIfFolded && !sites.length && !racks.length" class="hint">{{ t('sim.nosites') }}</span>

    <template v-if="active">
      <span v-if="ui.whatIfFolded" class="what" :title="label(active)">{{ label(active) }}</span>
      <span class="res" :class="stateTone">
        <span v-if="editor.sim?.loading" class="spinner" />
        <template v-else-if="res">
          <template v-if="!ui.whatIfFolded"><span v-for="c in chips" :key="c.text" class="c" :class="c.tone">{{ c.text }}</span></template>
          <b>{{ t('sim.result.' + res.state) }}</b>
        </template>
        <span v-else-if="editor.sim?.error" class="c crit">{{ editor.sim.error }}</span>
      </span>
      <button class="x" :title="t('sim.reset')" :aria-label="t('sim.reset')" @click="editor.exitSim()"><X :size="15" /></button>
    </template>
    <button v-if="!ui.whatIfFolded" class="w ghost icon" :title="t('sim.matrix.title')" :aria-label="t('sim.matrix')" data-testid="whatif-matrix" @click="ui.modal = 'matrix'"><Grid3x3 :size="14" /></button>
    <button class="w ghost icon" :title="ui.whatIfFolded ? t('sim.bar.unfold') : t('sim.bar.fold')" :aria-label="ui.whatIfFolded ? t('sim.bar.unfold') : t('sim.bar.fold')" :aria-expanded="!ui.whatIfFolded" data-testid="whatif-fold"
            @click="ui.whatIfFolded = !ui.whatIfFolded"><component :is="ui.whatIfFolded ? ChevronUp : ChevronDown" :size="14" /></button>
  </div>
</template>

<style scoped>
.whatif { position: absolute; left: 50%; bottom: 18px; transform: translateX(-50%); width: max-content; z-index: 60; display: flex; align-items: center; gap: 8px; padding: 9px 12px; background: var(--ink); color: var(--on-dark); border-radius: 12px; box-shadow: var(--shadow-3); max-width: calc(100% - 420px); flex-wrap: wrap; }
/* the strip of a narrow canvas (the breakpoint is the same in lib/useFloat's callers: the container of the editor) is for the docked bar; one the user has moved is a panel of its own */
@container (max-width: 1250px) { .whatif:not(.floating) { left: 18px; right: 18px; bottom: 62px; transform: none; width: auto; max-width: none; justify-content: center; } .whatif.folded:not(.floating) { right: auto; width: max-content; } }
.whatif.floating { max-width: min(640px, calc(100% - 24px)); width: max-content; }
.whatif { cursor: grab; touch-action: none; }
.whatif.dragging { box-shadow: var(--shadow-3), 0 0 0 2px var(--accent); user-select: none; cursor: grabbing; }
.whatif button, .whatif input, .whatif label, .whatif .menu { cursor: pointer; }
.whatif .find { cursor: text; }
.whatif.folded { padding: 6px 8px; gap: 6px; flex-wrap: nowrap; }
.grip { flex: none; width: 18px; height: 30px; margin: 0 -2px 0 -6px; padding: 0; border: 0; background: transparent; color: var(--on-dark-muted); border-radius: 5px; display: inline-flex; align-items: center; justify-content: center; user-select: none; }
.whatif .grip { cursor: grab; }
.grip:hover, .grip.on { color: #fff; background: var(--ink-3); }
.whatif .grip.on { cursor: grabbing; }
.lbl-btn { display: inline-flex; align-items: center; gap: 6px; height: 30px; padding: 0 6px; border: 0; background: transparent; color: var(--on-dark); font-size: 13px; white-space: nowrap; border-radius: 6px; }
.lbl-btn:hover { background: var(--ink-3); }
.what { max-width: 220px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12.5px; color: var(--on-dark-muted); }
.res { white-space: nowrap; }
.lbl { font-size: 13px; color: var(--on-dark-muted); margin-right: 2px; white-space: nowrap; }
.w { display: inline-flex; align-items: center; gap: 6px; height: 30px; padding: 0 12px; background: transparent; border: 1px solid #343b50; border-radius: 7px; color: var(--on-dark); font-size: 13px; white-space: nowrap; }
.w:hover { background: var(--ink-3); border-color: #4a5470; }
.w.on { background: var(--crit); border-color: var(--crit); color: #fff; font-weight: 600; }
.w.pick { background: var(--ink-3); border-color: var(--on-dark-muted); }
.w.ghost { border-color: transparent; color: var(--on-dark-muted); }
.w.icon { padding: 0; width: 30px; justify-content: center; }
.w.ghost:hover { color: #fff; }
.chev { opacity: .6; }
.pop { position: relative; }
.down .menu { bottom: auto; top: 38px; }
.menu { position: absolute; bottom: 38px; left: 0; min-width: 210px; max-width: 320px; background: var(--ink-2); border: 1px solid #343b50; border-radius: 8px; padding: 4px; display: flex; flex-direction: column; gap: 2px; box-shadow: var(--shadow-3); }
.menu.wide { min-width: 250px; }
.menu.flip { left: auto; right: 0; }
.items { display: flex; flex-direction: column; max-height: 280px; overflow: auto; }
.menu .items button { text-align: left; background: transparent; border: 0; color: var(--on-dark); padding: 7px 10px; border-radius: 5px; font-size: 13px; white-space: nowrap; }
.menu .items button:hover { background: var(--ink-3); }
.menu .items button.cur { color: #fff; background: var(--ink-3); font-weight: 600; }
.find { margin: 2px; height: 30px; padding: 0 10px; background: var(--ink); border: 1px solid #343b50; border-radius: 6px; color: var(--on-dark); font-size: 13px; }
.find::placeholder { color: var(--on-dark-muted); }
.mh { margin: 4px 8px 2px; font-size: 12px; color: var(--on-dark-muted); }
.chk { display: flex; align-items: center; gap: 8px; padding: 6px 10px; border-radius: 5px; font-size: 13px; cursor: pointer; }
.chk:hover { background: var(--ink-3); }
.go { flex: 1; height: 32px; border-radius: 6px; border: 0; background: var(--crit); color: #fff; font-weight: 600; font-size: 13px; }
.go:disabled { opacity: .45; }
.compose { min-width: 300px; max-width: 360px; }
.compose .items { max-height: 300px; }
.compose .gh { display: block; margin: 2px 10px 2px; font-size: 10.5px; letter-spacing: .08em; text-transform: uppercase; color: var(--on-dark-muted); font-weight: 600; }
.compose .grp + .grp .gh { margin-top: 8px; }
.compose .chk small { margin-left: auto; padding-left: 10px; color: var(--on-dark-muted); font-size: 11.5px; white-space: nowrap; }
.compose .more { margin: 4px 10px; font-size: 12px; color: var(--on-dark-muted); }
.addsel { display: flex; align-items: center; gap: 6px; margin: 2px; padding: 6px 10px; border: 1px dashed #4a5470; border-radius: 6px; background: transparent; color: var(--on-dark); font-size: 12.5px; text-align: left; }
.addsel:hover { background: var(--ink-3); }
.foot { display: flex; gap: 6px; margin: 4px 2px 2px; }
.clear { height: 32px; padding: 0 10px; border: 1px solid #343b50; border-radius: 6px; background: transparent; color: var(--on-dark-muted); font-size: 12.5px; }
.clear:hover { color: #fff; background: var(--ink-3); }
.hint { font-size: 12.5px; color: var(--on-dark-muted); }
.res { display: inline-flex; align-items: center; gap: 10px; height: 30px; padding: 0 12px; border-radius: 7px; font-family: var(--mono); font-size: 12.5px; background: rgba(39, 194, 94, .12); border: 1px solid rgba(39, 194, 94, .45); color: #7be5a3; }
.res.risk { background: rgba(243, 160, 12, .12); border-color: rgba(243, 160, 12, .5); color: #ffc266; }
.res.crit { background: rgba(192, 38, 45, .16); border-color: rgba(255, 107, 112, .5); color: #ff9a9e; }
.res .c.crit { color: #ff9a9e; } .res .c.risk { color: #ffc266; }
.res b { font-weight: 600; }
.x { border: 0; background: transparent; color: var(--on-dark-muted); display: inline-flex; padding: 4px; border-radius: 5px; }
.x:hover { color: #fff; background: var(--ink-3); }
</style>
