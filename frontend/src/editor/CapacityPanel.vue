<script setup lang="ts">
import { computed, ref } from 'vue'
import { Crosshair, Play } from 'lucide-vue-next'
import { t } from '@/i18n'
import { capacityView, failureHeadroom, loadHeadroom, stateCounts, type CapacityState, type CapacityView } from '@/lib/capacity'
import { fmtNumber } from '@/lib/format'
import { focusNeed } from '@/lib/navigate'
import { scenarioLabel } from '@/lib/sim'
import { useApp } from '@/stores/app'
import { useEditor } from '@/stores/editor'

const app = useApp()
const editor = useEditor()

/** "What if the load grows": a multiplier of the declared peak (client-side, the plan scales linearly). */
const factor = ref(1)
const PRESETS = [1, 1.5, 2, 3]

const report = computed(() => editor.report)
const rows = computed(() => report.value?.capacity ?? [])
const req = computed(() => editor.arch.requirements)
const minFailure = computed(() => app.boot?.engine.minCapacityDuringFailure ?? 0.7)
const headroomPct = computed(() => app.boot?.engine.headroomPercent ?? 30)

const ORDER: Record<CapacityState, number> = { over: 0, tight: 1, unknown: 2, ok: 3 }
const views = computed<CapacityView[]>(() =>
  rows.value.map((r) => capacityView(r, factor.value, minFailure.value)).sort((a, b) => ORDER[a.state] - ORDER[b.state] || (b.utilization ?? 0) - (a.utilization ?? 0)),
)
const counts = computed(() => stateCounts(views.value))
const room = computed(() => loadHeadroom(rows.value))
const roomFailure = computed(() => failureHeadroom(rows.value, minFailure.value))
const peak = computed(() => (req.value.rpsPeak ?? 0) * factor.value)

const fx = (n: number) => String(Math.round(n * 100) / 100)
const rps = (n: number) => fmtNumber(Math.round(n))
const pct = (ratio: number) => Math.round(ratio * 100)
const tone = (s: CapacityState) => (s === 'over' ? 'crit' : s === 'tight' ? 'risk' : s === 'unknown' ? 'nodata' : 'ok')
const tick = (v: CapacityView) => (v.row.targetRps > 0 ? Math.min(1, v.row.loadRps / v.row.targetRps) : 1)

function show(v: CapacityView) {
  editor.showSubjects({ subjects: { nodes: v.row.nodes, edges: [], zones: [] } })
}
function replay(v: CapacityView) {
  if (v.row.worst) void editor.runScenario(v.row.worst.scenario)
}
function fillRps(v: CapacityView) {
  focusNeed({ target: 'node', id: v.row.nodes[0], prop: 'rpsPerReplica' })
}
</script>

<template>
  <div class="cap">
    <div class="head">
      <h2 class="grow">{{ t('cap.title') }}</h2>
    </div>

    <div v-if="editor.isEmpty" class="empty">{{ t('checks.emptyArch') }}</div>
    <div v-else-if="!rows.length" class="empty col">
      <template v-if="!req.rpsPeak">
        <p>{{ t('cap.empty.rps') }}</p>
        <button v-if="app.canEdit && !editor.readonly" class="btn btn-sm btn-primary" @click="focusNeed({ target: 'requirements', prop: 'rpsPeak' })">{{ t('cap.empty.rps.fill') }}</button>
      </template>
      <p v-else>{{ t('cap.empty.none') }}</p>
    </div>

    <template v-else>
      <p class="intro">{{ t('cap.intro') }}</p>

      <section class="whatif" :aria-label="t('cap.whatif')">
        <div class="wl">
          <b>{{ t('cap.whatif') }}</b>
          <span class="mono val">×{{ fx(factor) }}<template v-if="peak"> = {{ rps(peak) }} rps</template></span>
        </div>
        <input v-model.number="factor" class="slider" type="range" min="1" max="4" step="0.05" :aria-label="t('cap.whatif')" />
        <div class="presets">
          <button v-for="p in PRESETS" :key="p" class="chip" :class="{ on: factor === p }" @click="factor = p">×{{ fx(p) }}</button>
          <span class="meta faint">
            <template v-if="req.rpsPeak">{{ t('cap.peak', { rps: rps(req.rpsPeak) }) }}</template>
            <template v-if="req.growthPercent"> · {{ t('cap.growth', { pct: req.growthPercent }) }}</template>
          </span>
        </div>
      </section>

      <div class="tiles">
        <div class="tile" :class="room && room.factor < factor ? 'crit' : room && room.factor < factor * 1.25 ? 'risk' : 'ok'" :title="room ? t('cap.headroom.hint', { name: room.row.name }) : ''">
          <span class="tl">{{ t('cap.headroom.title') }}</span>
          <b>{{ room ? '×' + fx(room.factor) : '—' }}</b>
          <span v-if="room" class="tn truncate">{{ room.row.name }}</span>
        </div>
        <div class="tile" :class="roomFailure && roomFailure.factor < factor ? 'crit' : roomFailure && roomFailure.factor < factor * 1.25 ? 'risk' : 'ok'" :title="roomFailure ? t('cap.failure.hint', { name: roomFailure.row.name, min: pct(minFailure) }) : ''">
          <span class="tl">{{ t('cap.failure.title') }}</span>
          <b>{{ roomFailure ? '×' + fx(roomFailure.factor) : '—' }}</b>
          <span v-if="roomFailure" class="tn truncate">{{ roomFailure.row.name }}</span>
        </div>
      </div>

      <div class="summary">
        <span v-if="counts.over" class="tag tag-crit">{{ t('cap.summary.over', { n: counts.over }) }}</span>
        <span v-if="counts.tight" class="tag tag-risk">{{ t('cap.summary.tight', { n: counts.tight }) }}</span>
        <span v-if="counts.unknown" class="tag tag-nodata">{{ t('cap.summary.unknown', { n: counts.unknown }) }}</span>
        <span v-if="counts.ok" class="tag tag-ok">{{ t('cap.summary.ok', { n: counts.ok }) }}</span>
      </div>

      <ul class="list">
        <li v-for="v in views" :key="v.row.unit" class="svc" :class="tone(v.state)">
          <div class="rh">
            <button class="name" :title="t('cap.show')" @click="show(v)"><Crosshair :size="13" /><b class="truncate">{{ v.row.name }}</b></button>
            <span class="inst faint">{{ t('cap.instances', { n: v.row.instances }) }}</span>
            <span class="grow" />
            <span class="tag" :class="'tag-' + tone(v.state)">{{ t('cap.state.' + v.state) }}</span>
          </div>

          <template v-if="v.state !== 'unknown'">
            <div class="bar" role="img" :aria-label="t('cap.load', { load: rps(v.load), cap: rps(v.row.capacityRps ?? 0) })">
              <i class="fill bar-fill" :style="{ width: Math.min(100, (v.utilization ?? 0) * 100) + '%' }" />
              <i class="tick" :style="{ left: tick(v) * 100 + '%' }" :title="t('cap.tick', { h: headroomPct })" />
            </div>
            <div class="nums">
              <span class="mono">{{ t('cap.load', { load: rps(v.load), cap: rps(v.row.capacityRps ?? 0) }) }}</span>
              <span class="mono faint">{{ pct(v.utilization ?? 0) }}%</span>
            </div>
            <div v-if="v.needed !== null" class="need" :class="{ more: (v.add ?? 0) > 0 }">
              {{ (v.add ?? 0) > 0 ? t('cap.needed.add', { n: v.needed, add: v.add }) : t('cap.needed.enough', { n: v.needed }) }}
            </div>
            <div v-if="v.worst" class="worst" :class="{ bad: !v.worst.ok }">
              <span class="grow">{{ t('cap.worst', { name: scenarioLabel(v.row.worst!.scenario, editor.shown, report?.units), cap: rps(v.worst.capacity), load: rps(v.worst.load), pct: pct(v.worst.ratio) }) }}</span>
              <button class="btn btn-ghost btn-icon" :title="t('cap.worst.replay')" :aria-label="t('cap.worst.replay')" @click="replay(v)"><Play :size="13" /></button>
            </div>
          </template>
          <div v-else class="nomodel">
            <span>{{ t('cap.nomodel', { load: rps(v.load) }) }}</span>
            <button v-if="app.canEdit && !editor.readonly" class="btn btn-sm" @click="fillRps(v)">{{ t('cap.nomodel.fill') }}</button>
          </div>
        </li>
      </ul>

      <p class="foot faint">{{ t('cap.footnote', { h: headroomPct, min: pct(minFailure) }) }}</p>
    </template>
  </div>
</template>

<style scoped>
.cap { display: flex; flex-direction: column; min-height: 0; flex: 1; overflow: auto; padding-bottom: 20px; }
.head { display: flex; align-items: baseline; padding: 16px 20px 4px; }
.head h2 { font-size: 16px; }
.intro { margin: 0; padding: 0 20px 10px; font-size: 12.5px; color: var(--muted); line-height: 1.5; }
.empty { padding: 24px 20px; color: var(--muted); display: flex; gap: 8px; align-items: center; justify-content: center; text-align: center; }
.empty.col { flex-direction: column; }
.empty p { margin: 0; }

.whatif { margin: 0 20px; padding: 12px 14px 12px; background: var(--surface); border: 1px solid var(--line-strong); border-radius: var(--radius); display: flex; flex-direction: column; gap: 8px; }
.wl { display: flex; align-items: baseline; justify-content: space-between; font-size: 13px; }
.val { color: var(--accent); font-weight: 600; }
.slider { width: 100%; accent-color: var(--accent); }
.presets { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
.presets .chip { height: 24px; padding: 0 10px; font-family: var(--mono); font-size: 12px; }
.presets .chip.on { background: var(--accent-soft); border-color: var(--accent-line); color: var(--accent); font-weight: 600; }
.meta { margin-left: auto; font-size: 12px; }

.tiles { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; padding: 10px 20px 0; }
.tile { display: flex; flex-direction: column; gap: 1px; padding: 9px 12px; border-radius: var(--radius); border: 1px solid transparent; min-width: 0; }
.tile .tl { font-size: 11.5px; }
.tile b { font-size: 22px; line-height: 1.2; letter-spacing: -0.02em; font-variant-numeric: tabular-nums; }
.tile .tn { font-size: 12px; }
.tile.ok { background: var(--ok-bg); color: var(--ok); }
.tile.risk { background: var(--risk-bg); color: var(--risk); }
.tile.crit { background: var(--crit-bg); color: var(--crit); }

.summary { display: flex; flex-wrap: wrap; gap: 6px; padding: 10px 20px 0; }
.list { list-style: none; margin: 12px 0 0; padding: 12px 20px 0; border-top: 1px solid var(--line); background: var(--surface-2); display: flex; flex-direction: column; gap: 10px; flex: 1; }
.svc { background: var(--surface); border: 1px solid var(--line-strong); border-radius: var(--radius-lg); padding: 12px 14px; display: flex; flex-direction: column; gap: 7px; }
.svc.crit { border-color: var(--crit-line); } .svc.risk { border-color: var(--risk-line); }
.rh { display: flex; align-items: center; gap: 8px; min-width: 0; }
.name { display: inline-flex; align-items: center; gap: 6px; min-width: 0; border: 0; background: transparent; padding: 2px 4px 2px 0; border-radius: 4px; color: var(--text); font-size: 14px; }
.name svg { color: var(--faint); flex: none; }
.name:hover svg { color: var(--accent); }
.inst { font-size: 12px; white-space: nowrap; }
.bar { position: relative; height: 8px; border-radius: 4px; background: var(--surface-sunken); overflow: visible; }
.bar .fill { position: absolute; inset: 0 auto 0 0; border-radius: 4px; background: var(--ok-dot); }
.svc.risk .bar .fill { background: var(--risk-dot); }
.svc.crit .bar .fill { background: var(--crit); }
.bar .tick { position: absolute; top: -3px; bottom: -3px; width: 2px; margin-left: -1px; background: var(--ink); opacity: .55; border-radius: 1px; }
.nums { display: flex; justify-content: space-between; font-size: 12.5px; color: var(--text-2); }
.need { font-size: 12.5px; color: var(--text-2); }
.need.more { color: var(--risk); font-weight: 600; }
.worst { display: flex; align-items: center; gap: 6px; padding: 6px 4px 6px 10px; background: var(--surface-2); border-radius: var(--radius-sm); font-size: 12.5px; color: var(--text-2); }
.worst.bad { background: var(--risk-bg); color: var(--risk); }
.worst .btn { flex: none; width: 26px; height: 26px; }
.nomodel { display: flex; align-items: center; gap: 10px; font-size: 12.5px; color: var(--muted); }
.nomodel span { flex: 1; line-height: 1.45; }
.foot { margin: 0; padding: 12px 20px 0; font-size: 12px; line-height: 1.5; }
</style>
