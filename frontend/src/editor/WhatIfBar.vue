<script setup lang="ts">
import { computed, ref } from 'vue'
import { Crosshair, Grid3x3, X } from 'lucide-vue-next'
import type { Scenario } from '@/api/types'
import { t } from '@/i18n'
import { resultChips, sameScenario, shortZone } from '@/lib/sim'
import { useEditor } from '@/stores/editor'
import { useUi } from '@/stores/ui'

const editor = useEditor()
const ui = useUi()
const menu = ref<'link' | 'sites' | 'geo' | null>(null)

const sites = computed(() => editor.siteZones)
const pairs = computed(() => {
  const out: { a: string; b: string; label: string }[] = []
  for (let i = 0; i < sites.value.length; i++) for (let j = i + 1; j < sites.value.length; j++) out.push({ a: sites.value[i].id, b: sites.value[j].id, label: `${shortZone(sites.value[i].name)} ↔ ${shortZone(sites.value[j].name)}` })
  return out
})
const geos = computed(() => [...new Set(editor.arch.zones.map((z) => z.props.geo).filter((g): g is string => typeof g === 'string' && !!g))])
const active = computed(() => editor.sim?.scenario ?? null)
const res = computed(() => editor.sim?.result)
const chips = computed(() => (res.value ? resultChips(res.value) : []))
const stateTone = computed(() => (res.value?.state === 'up' ? 'ok' : res.value?.state === 'degraded' ? 'risk' : 'crit'))

function run(sc: Scenario) {
  menu.value = null
  void editor.runScenario(sc)
}
function pickNode() {
  menu.value = null
  const s = editor.selection
  if (s?.type === 'node') return run({ type: 'node', nodeId: s.id })
  editor.pickingNode = !editor.pickingNode
}
const isNodeActive = computed(() => active.value?.type === 'node' || active.value?.type === 'instance')
</script>

<template>
  <div class="whatif" role="toolbar" :aria-label="t('sim.whatif')">
    <span class="lbl">{{ t('sim.whatif') }}</span>
    <button class="w" :class="{ on: isNodeActive, pick: editor.pickingNode }" @click="pickNode"><Crosshair :size="13" />{{ t('sim.node') }}</button>
    <button v-for="z in sites" :key="z.id" class="w" :class="{ on: sameScenario(active, { type: 'zone', zoneId: z.id }) }" @click="run({ type: 'zone', zoneId: z.id })">{{ shortZone(z.name) }}</button>
    <div v-if="pairs.length" class="pop">
      <button class="w" :class="{ on: active?.type === 'link' }" @click="menu = menu === 'link' ? null : 'link'">{{ t('sim.channel') }}</button>
      <div v-if="menu === 'link'" class="menu"><button v-for="p in pairs" :key="p.a + p.b" @click="run({ type: 'link', a: p.a, b: p.b })">{{ p.label }}</button></div>
    </div>
    <div v-if="pairs.length" class="pop">
      <button class="w" :class="{ on: active?.type === 'sites' }" @click="menu = menu === 'sites' ? null : 'sites'">{{ t('sim.sites2') }}</button>
      <div v-if="menu === 'sites'" class="menu"><button v-for="p in pairs" :key="p.a + p.b" @click="run({ type: 'sites', zoneIds: [p.a, p.b] })">{{ p.label.replace('↔', '+') }}</button></div>
    </div>
    <div v-if="geos.length" class="pop">
      <button class="w" :class="{ on: active?.type === 'geo' }" @click="menu = menu === 'geo' ? null : 'geo'">{{ t('sim.domain') }}</button>
      <div v-if="menu === 'geo'" class="menu"><button v-for="g in geos" :key="g" @click="run({ type: 'geo', geo: g })">{{ g }}</button></div>
    </div>
    <span v-if="!sites.length" class="hint">{{ t('sim.nosites') }}</span>

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
.pop { position: relative; }
.menu { position: absolute; bottom: 38px; left: 0; min-width: 190px; background: var(--ink-2); border: 1px solid #343b50; border-radius: 8px; padding: 4px; display: flex; flex-direction: column; box-shadow: var(--shadow-3); }
.menu button { text-align: left; background: transparent; border: 0; color: var(--on-dark); padding: 7px 10px; border-radius: 5px; font-size: 13px; white-space: nowrap; }
.menu button:hover { background: var(--ink-3); }
.hint { font-size: 12.5px; color: var(--on-dark-muted); }
.res { display: inline-flex; align-items: center; gap: 10px; height: 30px; padding: 0 12px; border-radius: 7px; font-family: var(--mono); font-size: 12.5px; background: rgba(39, 194, 94, .12); border: 1px solid rgba(39, 194, 94, .45); color: #7be5a3; }
.res.risk { background: rgba(243, 160, 12, .12); border-color: rgba(243, 160, 12, .5); color: #ffc266; }
.res.crit { background: rgba(192, 38, 45, .16); border-color: rgba(255, 107, 112, .5); color: #ff9a9e; }
.res .c.crit { color: #ff9a9e; } .res .c.risk { color: #ffc266; }
.res b { font-weight: 600; }
.x { border: 0; background: transparent; color: var(--on-dark-muted); display: inline-flex; padding: 4px; border-radius: 5px; }
.x:hover { color: #fff; background: var(--ink-3); }
</style>
