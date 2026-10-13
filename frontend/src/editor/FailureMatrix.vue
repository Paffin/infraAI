<script setup lang="ts">
import { computed, ref } from 'vue'
import { Search } from 'lucide-vue-next'
import Modal from '@/components/Modal.vue'
import { t } from '@/i18n'
import { fmtDuration, fmtPct } from '@/lib/format'
import { resultChips, scenarioFamily, scenarioLabel, type ScenarioFamily } from '@/lib/sim'
import { useApp } from '@/stores/app'
import { useEditor } from '@/stores/editor'
import { useUi } from '@/stores/ui'

const app = useApp()
const editor = useEditor()
const ui = useUi()

const STATE_RANK = { down: 0, degraded: 1, up: 2 } as const
const FAMILIES: ScenarioFamily[] = ['zone', 'rack', 'isolate', 'link', 'sites', 'geo']
const PAGE = 80

const family = ref<ScenarioFamily | 'all'>('all')
const badOnly = ref(false)
const query = ref('')
const limit = ref(PAGE)

const all = computed(() =>
  (editor.report?.scenarios ?? []).map((s, i) => ({
    i,
    s,
    family: scenarioFamily(s.scenario, editor.arch, app.index!),
    // a whole region lists every one of its sites; its name says more
    label: s.failed.length && s.scenario.type !== 'geo' ? s.failed.join(' + ') : scenarioLabel(s.scenario, editor.arch, editor.report?.units),
    bad: s.state !== 'up' || s.splitBrain || s.dataLost,
    tone: s.state === 'up' ? 'tag-ok' : s.state === 'degraded' ? 'tag-risk' : 'tag-crit',
    flags: resultChips({ state: s.state, recovery: { kind: s.recoveryKind, seconds: s.recoverySeconds }, rpoSeconds: s.rpoSeconds, rpoAssumed: s.rpoAssumed, capacityRatio: s.capacityRatio, splitBrain: s.splitBrain, dataLost: s.dataLost }).filter((c) => c.tone === 'crit'),
  })),
)
const counts = computed(() => {
  const out: Record<string, number> = { all: all.value.length }
  for (const r of all.value) out[r.family] = (out[r.family] ?? 0) + 1
  return out
})
const present = computed(() => FAMILIES.filter((f) => counts.value[f]))
const rows = computed(() => {
  const needle = query.value.trim().toLowerCase()
  return all.value
    .filter((r) => (family.value === 'all' || r.family === family.value) && (!badOnly.value || r.bad) && (!needle || r.label.toLowerCase().includes(needle)))
    .sort((a, b) => STATE_RANK[a.s.state] - STATE_RANK[b.s.state] || a.i - b.i)
})
const stats = computed(() => editor.report?.scenarioStats)

function replay(r: { s: { scenario: import('@/api/types').Scenario } }) {
  ui.modal = null
  void editor.runScenario(r.s.scenario)
}
function pick(f: ScenarioFamily | 'all') {
  family.value = f
  limit.value = PAGE
}
const recovery = (k: string, sec: number | null) => (k === 'auto' ? t('sim.result.switch', { s: fmtDuration(sec) }) : k === 'manual' ? t('sim.result.manual', { s: fmtDuration(sec) }) : k === 'never' ? t('sim.result.never') : '—')
</script>

<template>
  <Modal :title="t('sim.matrix.title')" size="lg" @close="ui.modal = null">
    <p v-if="!all.length" class="muted">{{ t('sim.matrix.empty') }}</p>
    <template v-else>
      <div class="bar">
        <div class="chips" role="group">
          <button class="chip" :class="{ on: family === 'all' }" :aria-pressed="family === 'all'" @click="pick('all')">{{ t('sim.filter.all') }} <i>{{ counts.all }}</i></button>
          <button v-for="f in present" :key="f" class="chip" :class="{ on: family === f }" :aria-pressed="family === f" @click="pick(f)">{{ t('sim.filter.' + f) }} <i>{{ counts[f] }}</i></button>
        </div>
        <label class="bad chip" :class="{ on: badOnly }"><input v-model="badOnly" type="checkbox" />{{ t('sim.filter.bad') }}</label>
        <div v-if="all.length > 12" class="find"><Search :size="14" /><input v-model="query" class="input" :placeholder="t('sim.matrix.search')" @input="limit = PAGE" /></div>
      </div>
      <p v-if="stats" class="stats muted">
        {{ t('sim.matrix.stats', { sites: stats.sites, racks: stats.racks, listed: stats.listed, generated: stats.generated }) }}
        <span v-if="stats.truncated" class="warn">{{ t('sim.matrix.truncated') }}</span>
      </p>
      <table class="table">
        <thead><tr><th>{{ t('sim.col.scenario') }}</th><th>{{ t('sim.col.state') }}</th><th>{{ t('sim.col.recovery') }}</th><th>{{ t('sim.col.rpo') }}</th><th>{{ t('sim.col.capacity') }}</th><th /></tr></thead>
        <tbody>
          <tr v-for="r in rows.slice(0, limit)" :key="r.i" class="clickable" tabindex="0" @click="replay(r)" @keydown.enter="replay(r)">
            <td><b :title="r.label" class="lab">{{ r.label }}</b><span v-if="family === 'all' && r.family !== 'zone'" class="fam">{{ t('sim.filter.' + r.family) }}</span></td>
            <td><span class="tag" :class="r.tone">{{ t('sim.result.' + r.s.state) }}</span></td>
            <td>{{ recovery(r.s.recoveryKind, r.s.recoverySeconds) }}</td>
            <td class="mono">{{ r.s.rpoSeconds === null ? '?' : r.s.rpoSeconds === 0 ? '0' : fmtDuration(r.s.rpoSeconds) }}<span v-if="r.s.rpoAssumed && r.s.rpoSeconds" :title="t('sim.assumed')"> ~</span></td>
            <td class="mono">{{ fmtPct(r.s.capacityRatio) }}</td>
            <td><span v-for="f in r.flags" :key="f.text" class="tag tag-crit" style="margin-right: 4px">{{ f.text }}</span></td>
          </tr>
        </tbody>
      </table>
      <div class="more">
        <span class="muted">{{ t('sim.matrix.shown', { n: Math.min(limit, rows.length), total: rows.length }) }}</span>
        <button v-if="rows.length > limit" class="btn btn-sm" @click="limit += PAGE">{{ t('common.more') }}</button>
      </div>
    </template>
  </Modal>
</template>

<style scoped>
.bar { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; margin-bottom: 8px; }
.chips { display: flex; gap: 6px; flex-wrap: wrap; flex: 1; }
.chip i { font-style: normal; color: var(--faint); margin-left: 4px; font-size: 11.5px; }
.chip.on { background: var(--accent-soft); border-color: var(--accent-line); color: var(--accent); font-weight: 600; }
.chip.on i { color: var(--accent); }
.bad input { position: absolute; opacity: 0; pointer-events: none; }
.bad { cursor: pointer; }
.find { position: relative; width: 220px; }
.find svg { position: absolute; left: 10px; top: 10px; color: var(--faint); }
.find .input { padding-left: 30px; height: 32px; }
.stats { font-size: 12.5px; margin: 0 0 8px; }
.warn { display: block; color: var(--risk); margin-top: 2px; }
.clickable { cursor: pointer; }
.clickable:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
.tag { white-space: nowrap; }
.lab { display: inline-block; max-width: 360px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; vertical-align: bottom; }
.fam { margin-left: 8px; font-size: 11px; color: var(--faint); font-weight: 400; }
.more { display: flex; align-items: center; justify-content: space-between; margin-top: 10px; font-size: 12.5px; }
</style>
