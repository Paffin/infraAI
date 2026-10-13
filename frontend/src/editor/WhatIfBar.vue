<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { ChevronUp, Crosshair, Grid3x3, X } from 'lucide-vue-next'
import type { Scenario } from '@/api/types'
import { t } from '@/i18n'
import { resultChips, sameScenario, scenarioLabel, shortZone } from '@/lib/sim'
import { useEditor } from '@/stores/editor'
import { useUi } from '@/stores/ui'

const editor = useEditor()
const ui = useUi()
type Menu = 'sites' | 'racks' | 'link' | 'isolate' | 'multi' | 'geo'
const menu = ref<Menu | null>(null)
const q = ref('')
const picked = ref<string[]>([])
const bar = ref<HTMLElement | null>(null)

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
  const n = menu.value === 'sites' || menu.value === 'multi' ? sites.value.length : menu.value === 'racks' ? racks.value.length : menu.value === 'link' ? links.value.length : menu.value === 'isolate' ? isolates.value.length : 0
  return n > 8
})

function toggle(m: Menu) {
  menu.value = menu.value === m ? null : m
  q.value = ''
  if (m === 'multi') picked.value = []
}
function run(sc: Scenario) {
  menu.value = null
  void editor.runScenario(sc)
}
function runPicked() {
  if (picked.value.length < 1) return
  run(picked.value.length === 1 ? { type: 'zone', zoneId: picked.value[0] } : { type: 'sites', zoneIds: [...picked.value] })
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
  <div ref="bar" class="whatif" data-overlay="bottom" data-tour="whatif" role="toolbar" :aria-label="t('sim.whatif')" @keydown.esc="menu = null">
    <span class="lbl">{{ t('sim.whatif') }}</span>
    <button class="w" :class="{ on: isNodeActive, pick: editor.pickingNode }" @click="pickNode"><Crosshair :size="13" />{{ t('sim.node') }}</button>

    <template v-if="sites.length <= INLINE_SITES">
      <button v-for="z in sites" :key="z.id" class="w" :class="{ on: isZoneActive(z.id) }" @click="run({ type: 'zone', zoneId: z.id })">{{ shortZone(z.name) }}</button>
    </template>
    <div v-else class="pop">
      <button class="w" :class="{ on: sites.some((z) => isZoneActive(z.id)) }" @click="toggle('sites')">{{ t('sim.sites') }} · {{ sites.length }}<ChevronUp :size="12" class="chev" /></button>
      <div v-if="menu === 'sites'" class="menu">
        <input v-if="showSearch" v-model="q" class="find" :placeholder="t('sim.sites.search')" autofocus />
        <div class="items"><button v-for="z in siteItems" :key="z.id" :class="{ cur: isZoneActive(z.id) }" @click="run({ type: 'zone', zoneId: z.id })">{{ z.name }}</button></div>
      </div>
    </div>

    <div v-if="racks.length" class="pop">
      <button class="w" :class="{ on: racks.some((z) => isZoneActive(z.id)) }" @click="toggle('racks')">{{ t('sim.rack') }} · {{ racks.length }}<ChevronUp :size="12" class="chev" /></button>
      <div v-if="menu === 'racks'" class="menu">
        <input v-if="showSearch" v-model="q" class="find" :placeholder="t('sim.sites.search')" />
        <div class="items"><button v-for="z in rackItems" :key="z.id" :class="{ cur: isZoneActive(z.id) }" @click="run({ type: 'zone', zoneId: z.id })">{{ z.name }}</button></div>
      </div>
    </div>

    <div v-if="links.length" class="pop">
      <button class="w" :class="{ on: active?.type === 'link' }" @click="toggle('link')">{{ t('sim.channel') }}<ChevronUp :size="12" class="chev" /></button>
      <div v-if="menu === 'link'" class="menu">
        <input v-if="showSearch" v-model="q" class="find" :placeholder="t('sim.sites.search')" />
        <div class="items"><button v-for="sc in linkItems" :key="JSON.stringify(sc)" :class="{ cur: sameScenario(active, sc) }" @click="run(sc)">{{ label(sc) }}</button></div>
      </div>
    </div>

    <div v-if="isolates.length" class="pop">
      <button class="w" :class="{ on: active?.type === 'isolate' }" :title="t('sim.isolate.hint')" @click="toggle('isolate')">{{ t('sim.isolate') }}<ChevronUp :size="12" class="chev" /></button>
      <div v-if="menu === 'isolate'" class="menu">
        <input v-if="showSearch" v-model="q" class="find" :placeholder="t('sim.sites.search')" />
        <div class="items"><button v-for="sc in isolateItems" :key="JSON.stringify(sc)" :class="{ cur: sameScenario(active, sc) }" @click="run(sc)">{{ label(sc) }}</button></div>
      </div>
    </div>

    <div v-if="sites.length >= 3" class="pop">
      <button class="w" :class="{ on: active?.type === 'sites' }" @click="toggle('multi')">{{ t('sim.pickSites') }}<ChevronUp :size="12" class="chev" /></button>
      <div v-if="menu === 'multi'" class="menu wide">
        <p class="mh">{{ t('sim.pickSites.hint') }}</p>
        <input v-if="showSearch" v-model="q" class="find" :placeholder="t('sim.sites.search')" />
        <div class="items">
          <label v-for="z in siteItems" :key="z.id" class="chk"><input v-model="picked" type="checkbox" :value="z.id" />{{ z.name }}</label>
        </div>
        <button class="go" :disabled="!picked.length" @click="runPicked">{{ t('sim.pickSites.run') }}<template v-if="picked.length"> · {{ picked.length }}</template></button>
      </div>
    </div>

    <div v-if="geos.length" class="pop">
      <button class="w" :class="{ on: active?.type === 'geo' }" @click="toggle('geo')">{{ t('sim.domain') }}<ChevronUp :size="12" class="chev" /></button>
      <div v-if="menu === 'geo'" class="menu"><div class="items"><button v-for="g in geos" :key="g" @click="run({ type: 'geo', geo: g })">{{ g }}</button></div></div>
    </div>
    <span v-if="!sites.length && !racks.length" class="hint">{{ t('sim.nosites') }}</span>

    <template v-if="active">
      <span class="res" :class="stateTone">
        <span v-if="editor.sim?.loading" class="spinner" />
        <template v-else-if="res">
          <span v-for="c in chips" :key="c.text" class="c" :class="c.tone">{{ c.text }}</span>
          <b>{{ t('sim.result.' + res.state) }}</b>
        </template>
        <span v-else-if="editor.sim?.error" class="c crit">{{ editor.sim.error }}</span>
      </span>
      <button class="x" :title="t('sim.reset')" :aria-label="t('sim.reset')" @click="editor.exitSim()"><X :size="15" /></button>
    </template>
    <button class="w ghost icon" :title="t('sim.matrix.title')" :aria-label="t('sim.matrix')" @click="ui.modal = 'matrix'"><Grid3x3 :size="14" /></button>
  </div>
</template>

<style scoped>
.whatif { position: absolute; left: 50%; bottom: 18px; transform: translateX(-50%); width: max-content; z-index: 60; display: flex; align-items: center; gap: 8px; padding: 9px 12px; background: var(--ink); color: var(--on-dark); border-radius: 12px; box-shadow: var(--shadow-3); max-width: calc(100% - 420px); flex-wrap: wrap; }
@container (max-width: 1250px) { .whatif { left: 18px; right: 18px; bottom: 62px; transform: none; width: auto; max-width: none; justify-content: center; } }
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
.menu { position: absolute; bottom: 38px; left: 0; min-width: 210px; max-width: 320px; background: var(--ink-2); border: 1px solid #343b50; border-radius: 8px; padding: 4px; display: flex; flex-direction: column; gap: 2px; box-shadow: var(--shadow-3); }
.menu.wide { min-width: 250px; }
.items { display: flex; flex-direction: column; max-height: 280px; overflow: auto; }
.menu .items button { text-align: left; background: transparent; border: 0; color: var(--on-dark); padding: 7px 10px; border-radius: 5px; font-size: 13px; white-space: nowrap; }
.menu .items button:hover { background: var(--ink-3); }
.menu .items button.cur { color: #fff; background: var(--ink-3); font-weight: 600; }
.find { margin: 2px; height: 30px; padding: 0 10px; background: var(--ink); border: 1px solid #343b50; border-radius: 6px; color: var(--on-dark); font-size: 13px; }
.find::placeholder { color: var(--on-dark-muted); }
.mh { margin: 4px 8px 2px; font-size: 12px; color: var(--on-dark-muted); }
.chk { display: flex; align-items: center; gap: 8px; padding: 6px 10px; border-radius: 5px; font-size: 13px; cursor: pointer; }
.chk:hover { background: var(--ink-3); }
.go { margin: 4px 2px 2px; height: 32px; border-radius: 6px; border: 0; background: var(--crit); color: #fff; font-weight: 600; font-size: 13px; }
.go:disabled { opacity: .45; }
.hint { font-size: 12.5px; color: var(--on-dark-muted); }
.res { display: inline-flex; align-items: center; gap: 10px; height: 30px; padding: 0 12px; border-radius: 7px; font-family: var(--mono); font-size: 12.5px; background: rgba(39, 194, 94, .12); border: 1px solid rgba(39, 194, 94, .45); color: #7be5a3; }
.res.risk { background: rgba(243, 160, 12, .12); border-color: rgba(243, 160, 12, .5); color: #ffc266; }
.res.crit { background: rgba(192, 38, 45, .16); border-color: rgba(255, 107, 112, .5); color: #ff9a9e; }
.res .c.crit { color: #ff9a9e; } .res .c.risk { color: #ffc266; }
.res b { font-weight: 600; }
.x { border: 0; background: transparent; color: var(--on-dark-muted); display: inline-flex; padding: 4px; border-radius: 5px; }
.x:hover { color: #fff; background: var(--ink-3); }
</style>
