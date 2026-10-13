<script setup lang="ts">
import { computed } from 'vue'
import { Crosshair, Zap } from 'lucide-vue-next'
import AnimatedNumber from '@/components/AnimatedNumber.vue'
import type { MapChip } from '@/lib/placementMap'
import { lt, t } from '@/i18n'
import { statusColor, statusRank } from '@/lib/findings'
import { buildSiteMap, ROLE_MARK } from '@/lib/placementMap'
import { siteScenario, sitesOf } from '@/lib/sites'
import { resultChips, stateKey } from '@/lib/sim'
import { useApp } from '@/stores/app'
import { useEditor } from '@/stores/editor'
import type { Status } from '@/api/types'

const app = useApp()
const editor = useEditor()

const sites = computed(() => (app.index ? sitesOf(editor.shown, app.index) : []))
const map = computed(() => (app.index ? buildSiteMap(editor.shown, sites.value, app.index) : { global: [], domains: [] }))
const total = computed(() => sites.value.length)

const kindName = (c: MapChip) => lt(app.index?.kinds.get(c.kind)?.name) || c.kind
const tip = (c: MapChip) => [kindName(c), c.role, c.count > 1 ? `×${c.count}` : ''].filter(Boolean).join(' · ')

/** The worst open finding that touches the site: the zone itself, its racks or any card in them. */
function worst(id: string, racks: string[]): Status | null {
  const zones = new Set([id, ...racks])
  let w: Status | null = editor.zoneStatus.get(id) ?? null
  for (const n of editor.shown.nodes) {
    if (!n.zoneId || !zones.has(n.zoneId)) continue
    const s = editor.nodeStatus.get(n.id)
    if (s && (!w || statusRank[s] < statusRank[w])) w = s
  }
  return w
}

const cards = computed(() => {
  const racks = new Map(sites.value.map((s) => [s.id, s.racks]))
  return map.value.domains.map((d) => ({
    geo: d.geo,
    sites: d.sites.map((s) => {
      const sc = siteScenario(editor.report, s.id)
      return {
        ...s,
        status: worst(s.id, racks.get(s.id) ?? []),
        scenario: sc,
        chipsOf: sc ? resultChips({ state: sc.state, recovery: { kind: sc.recoveryKind, seconds: sc.recoverySeconds }, rpoSeconds: sc.rpoSeconds, rpoAssumed: sc.rpoAssumed, capacityRatio: sc.capacityRatio, splitBrain: sc.splitBrain, dataLost: sc.dataLost }) : [],
      }
    }),
  }))
})

const failing = computed(() => (editor.sim?.scenario.type === 'zone' ? editor.sim.scenario.zoneId : null))

function show(id: string) {
  editor.select({ type: 'zone', id })
  editor.revealElements([], [id])
}
function fail(id: string) {
  void editor.runScenario({ type: 'zone', zoneId: id })
}
</script>

<template>
  <div class="sites">
    <div class="head">
      <h2 class="grow">{{ t('sites.title') }}</h2>
      <span v-if="total" class="count"><AnimatedNumber :value="total" /></span>
    </div>
    <div v-if="editor.isEmpty" class="empty">{{ t('sites.empty.arch') }}</div>
    <div v-else-if="total < 2" class="empty" data-testid="sites-few">{{ t('sites.few') }}</div>
    <template v-else>
      <p class="intro">{{ t('sites.intro') }}</p>
      <div v-if="map.global.length" class="global">
        <span v-for="c in map.global" :key="c.key" class="chip-s" :style="{ '--c': c.color }" :title="tip(c)">{{ c.abbr }}<em v-if="c.count > 1">×{{ c.count }}</em></span>
        <span class="gl">{{ t('placement.map.global') }}</span>
      </div>
      <section v-for="d in cards" :key="d.geo" class="domain">
        <h3 class="mono">{{ d.geo || t('sites.nogeo') }}<span>{{ d.sites.length }}</span></h3>
        <article v-for="(s, i) in d.sites" :key="s.id" class="site" :class="[s.role, failing === s.id && 'on']" :style="{ '--i': i }" :data-site="s.id">
          <header>
            <span v-if="s.status" class="sdot" :style="{ background: statusColor(s.status) }" :title="t('checks.statuses.' + s.status)" />
            <b class="truncate">{{ s.name }}</b>
            <span v-if="s.role !== 'working'" class="role">{{ t('placement.role.' + s.role) }}</span>
            <span class="grow" />
            <span class="nn" :title="t('sites.nodes')">{{ s.nodes }}</span>
          </header>
          <div v-if="s.chips.length" class="chips">
            <span v-for="c in s.chips" :key="c.key" class="chip-s" :class="{ w: c.role === 'witness' }" :style="{ '--c': c.color }" :title="tip(c)">{{ c.abbr }}<sup v-if="c.role && ROLE_MARK[c.role]">{{ ROLE_MARK[c.role] }}</sup><em v-if="c.count > 1">×{{ c.count }}</em></span>
          </div>
          <div v-for="(r, ri) in s.racks" :key="r.id" class="rack">
            <span class="rn mono">{{ r.name || t('placement.rack', { n: ri + 1 }) }}</span>
            <span v-for="c in r.chips" :key="c.key" class="chip-s" :class="{ w: c.role === 'witness' }" :style="{ '--c': c.color }" :title="tip(c)">{{ c.abbr }}<sup v-if="c.role && ROLE_MARK[c.role]">{{ ROLE_MARK[c.role] }}</sup><em v-if="c.count > 1">×{{ c.count }}</em></span>
          </div>
          <div v-if="s.scenario" class="verdict" :data-state="s.scenario.state">
            <span class="lose">{{ t('sites.if') }}</span>
            <span class="state" :class="s.scenario.state">{{ t(stateKey(s.scenario.state)) }}</span>
            <span v-for="c in s.chipsOf" :key="c.text" class="rc" :class="c.tone">{{ c.text }}</span>
          </div>
          <div class="acts">
            <button class="btn btn-sm btn-ghost" :title="t('sites.show.hint')" @click="show(s.id)"><Crosshair :size="13" />{{ t('sites.show') }}</button>
            <button class="btn btn-sm btn-ghost" :title="t('sites.fail.hint')" @click="fail(s.id)"><Zap :size="13" />{{ t('sites.fail') }}</button>
          </div>
        </article>
      </section>
    </template>
  </div>
</template>

<style scoped>
.sites { display: flex; flex-direction: column; min-height: 0; flex: 1; overflow: auto; padding-bottom: 20px; }
.head { display: flex; align-items: baseline; gap: 8px; padding: 16px 20px 4px; }
.head h2 { font-size: 16px; }
.count { font-size: 12px; color: var(--muted); background: var(--surface-sunken); border-radius: 10px; padding: 0 8px; }
.intro { margin: 0; padding: 0 20px 10px; font-size: 12.5px; color: var(--muted); line-height: 1.5; }
.empty { padding: 24px 20px; color: var(--muted); display: flex; gap: 8px; align-items: center; justify-content: center; text-align: center; }
.global { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; margin: 0 20px 12px; padding: 8px 10px; border: 1px dashed var(--line-strong); border-radius: var(--radius); background: var(--surface-2); }
.gl { font-size: 11.5px; color: var(--faint); margin-left: 4px; }
.domain { margin: 0 20px 14px; display: flex; flex-direction: column; gap: 8px; }
.domain h3 { margin: 0; font-size: 11.5px; font-weight: 600; color: var(--muted); display: flex; align-items: center; gap: 8px; letter-spacing: .02em; }
.domain h3 span { font-family: var(--font); background: var(--surface-sunken); border-radius: 10px; padding: 0 7px; font-size: 11px; color: var(--faint); }
.site { border: 1px solid var(--line-strong); border-radius: var(--radius); padding: 10px 12px 8px; background: var(--surface); display: flex; flex-direction: column; gap: 8px; min-width: 0; animation: site-in var(--dur-base) var(--ease-out) both; animation-delay: calc(min(var(--i, 0), 8) * var(--stagger)); transition: border-color var(--dur-fast), box-shadow var(--dur-fast); }
.site.on { border-color: var(--crit); box-shadow: 0 0 0 3px var(--crit-bg); }
.site.witness, .site.backup { border-style: dashed; background: var(--surface-2); }
.site.standby { border-color: var(--risk-line); background: var(--risk-bg); }
.site header { display: flex; align-items: center; gap: 8px; font-size: 13.5px; min-width: 0; }
.site header b { font-weight: 600; min-width: 0; }
.sdot { width: 9px; height: 9px; border-radius: 50%; flex: none; }
.role { font-size: 10.5px; padding: 1px 7px; border-radius: 9px; background: var(--surface-sunken); color: var(--muted); white-space: nowrap; }
.standby .role { background: #fff; color: var(--risk); }
.nn { font-size: 11.5px; color: var(--faint); font-variant-numeric: tabular-nums; }
.chips, .rack { display: flex; flex-wrap: wrap; gap: 4px; align-items: center; }
.rack { padding-top: 6px; border-top: 1px dashed var(--line); }
.rn { font-size: 10.5px; color: var(--faint); width: 100%; }
.chip-s { --c: #8b93a1; display: inline-flex; gap: 1px; height: 22px; padding: 0 6px; align-items: center; border-radius: 5px; font: 600 10.5px/1 var(--mono); letter-spacing: .02em; color: color-mix(in srgb, var(--c) 72%, #000); background: color-mix(in srgb, var(--c) 13%, #fff); border: 1px solid color-mix(in srgb, var(--c) 45%, #fff); }
.chip-s.w { border-style: dashed; }
.chip-s sup { font-size: 8.5px; margin-left: 2px; opacity: .85; }
.chip-s em { font-style: normal; font-size: 9.5px; opacity: .75; margin-left: 2px; }
.verdict { display: flex; flex-wrap: wrap; align-items: center; gap: 5px; padding: 7px 0 0; border-top: 1px solid var(--line); }
.lose { font-size: 11.5px; color: var(--muted); margin-right: 2px; }
.state { font-size: 11.5px; font-weight: 650; padding: 1px 8px; border-radius: 9px; }
.state.up { background: var(--ok-bg); color: var(--ok); }
.state.degraded { background: var(--risk-bg); color: var(--risk); }
.state.down { background: var(--crit-bg); color: var(--crit); }
.rc { font-size: 11.5px; padding: 1px 7px; border-radius: 9px; background: var(--surface-sunken); color: var(--text-2); white-space: nowrap; }
.rc.ok { background: var(--ok-bg); color: var(--ok); }
.rc.risk { background: var(--risk-bg); color: var(--risk); }
.rc.crit { background: var(--crit-bg); color: var(--crit); font-weight: 650; }
.acts { display: flex; gap: 4px; margin: 0 -6px; }
.acts .btn { color: var(--muted); }
.acts .btn:hover:not(:disabled) { color: var(--text); }
@keyframes site-in { from { opacity: 0; transform: translateY(var(--move)) scale(.98); } to { opacity: 1; transform: none; } }
</style>
