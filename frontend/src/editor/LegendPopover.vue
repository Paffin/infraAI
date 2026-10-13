<script setup lang="ts">
// "Обозначения": what the lines, the cards, the zones and — while a failure is shown — the particles and tags on the diagram mean.
// The links come from the catalog (every link type there is, with the way it is drawn); the rest is what the drawing itself does.
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { Cloud, Info } from 'lucide-vue-next'
import { lt, t } from '@/i18n'
import { statusColor } from '@/lib/findings'
import { useApp } from '@/stores/app'

const app = useApp()
const open = ref(false)
const root = ref<HTMLElement | null>(null)

const links = computed(() => (app.boot?.catalog.edgeTypes ?? []).map((e) => ({ id: e.id, name: lt(e.name), says: e.description ? lt(e.description) : t('legend.sem.' + e.semantics), color: e.style.color, dash: e.style.dash ?? '', width: Math.max(1.5, e.style.width ?? 1.6) })))
const statuses = ['ok', 'risk', 'critical', 'no_data'] as const

function outside(e: MouseEvent) {
  if (open.value && root.value && !root.value.contains(e.target as Node)) open.value = false
}
// capture phase: the canvas stops the mouse events of its own area (the zoom behind it), so a click on the diagram never reaches a listener that waits for it to bubble
onMounted(() => document.addEventListener('mousedown', outside, true))
onBeforeUnmount(() => document.removeEventListener('mousedown', outside, true))
</script>

<template>
  <div ref="root" class="legend-wrap" @keydown.esc="open = false">
    <button class="tool" :class="{ on: open }" aria-haspopup="dialog" :aria-expanded="open" data-testid="legend-toggle" :title="t('legend.hint')" @click="open = !open"><Info :size="14" />{{ t('legend.title') }}</button>
    <Transition name="pop">
      <div v-if="open" class="legend-pop" role="dialog" :aria-label="t('legend.title')" data-testid="legend">
        <section>
          <h3>{{ t('legend.links') }}</h3>
          <ul class="rows">
            <li v-for="l in links" :key="l.id">
              <svg class="line" width="38" height="10" aria-hidden="true"><line x1="1" y1="5" x2="31" y2="5" :stroke="l.color" :stroke-width="l.width" :stroke-dasharray="l.dash" stroke-linecap="round" /><path d="M29,1.5 L36,5 L29,8.5 Z" :fill="l.color" /></svg>
              <span class="tx"><b>{{ l.name }}</b>{{ l.says }}</span>
            </li>
          </ul>
        </section>

        <section>
          <h3>{{ t('legend.cards') }}</h3>
          <ul class="rows">
            <li>
              <span class="sw"><i v-for="s in statuses" :key="s" class="d" :style="{ background: statusColor(s) }" /></span>
              <span class="tx">{{ t('legend.dots') }}</span>
            </li>
            <li>
              <span class="sw"><span class="mini stack"><i /><i /><b /></span></span>
              <span class="tx">{{ t('legend.stack') }}</span>
            </li>
            <li>
              <span class="sw"><span class="mini"><em class="cloud"><Cloud :size="10" /></em></span></span>
              <span class="tx">{{ t('legend.managed') }}</span>
            </li>
            <li>
              <span class="sw"><span class="mini proposed" /></span>
              <span class="tx">{{ t('legend.proposed') }}</span>
            </li>
            <li>
              <span class="sw"><span class="zp active">active</span><span class="zp standby">standby</span><span class="zp witness">witness</span></span>
              <span class="tx">{{ t('legend.roles') }}</span>
            </li>
          </ul>
        </section>

        <section>
          <h3>{{ t('legend.failure') }}</h3>
          <ul class="rows">
            <li><span class="sw"><span class="band flow" /><i class="pt flow" /></span><span class="tx">{{ t('legend.flow') }}</span></li>
            <li><span class="sw"><span class="band lost" /><i class="pt lost" /></span><span class="tx">{{ t('legend.lost') }}</span></li>
            <li><span class="sw"><span class="band missed" /><i class="pt missed" /></span><span class="tx">{{ t('legend.missed') }}</span></li>
            <li><span class="sw"><span class="band detour" /><i class="pt detour" /></span><span class="tx">{{ t('legend.detour') }}</span></li>
            <li><span class="sw"><span class="tagx up">×2</span><span class="tagx none">—</span></span><span class="tx">{{ t('legend.load') }}</span></li>
            <li><span class="sw"><span class="tagx st down">{{ t('node.state.down') }}</span><span class="tagx st wait">{{ t('node.state.stalled') }}</span></span><span class="tx">{{ t('legend.states') }}</span></li>
          </ul>
        </section>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.legend-wrap { position: relative; display: inline-flex; }
.legend-pop { position: absolute; top: calc(100% + 8px); right: 0; width: 540px; max-width: calc(100vw - 32px); max-height: min(70vh, 640px); overflow: auto; z-index: 75; background: var(--surface); border: 1px solid var(--line-strong); border-radius: 14px; box-shadow: var(--shadow-3); padding: 14px 16px 12px; text-align: left; }
h3 { margin: 10px 0 6px; font-size: 10.5px; letter-spacing: .09em; text-transform: uppercase; color: var(--label); font-weight: 600; }
section:first-child h3 { margin-top: 0; }
.rows { list-style: none; margin: 0; padding: 0; display: grid; grid-template-columns: max-content 1fr; column-gap: 16px; row-gap: 9px; align-items: center; }
.rows li { display: contents; }
.tx { font-size: 12.5px; color: var(--text-2); line-height: 1.42; }
.tx b { display: block; color: var(--text); font-weight: 600; }
.sw { display: inline-flex; align-items: center; gap: 6px; }
.line { flex: none; }
.sw .d { width: 10px; height: 10px; border-radius: 50%; display: inline-block; }
.mini { position: relative; width: 44px; height: 22px; border-radius: 6px; background: var(--surface); border: 1px solid var(--line-strong); display: inline-block; box-shadow: var(--shadow-1); }
.mini.stack { background: none; border: 0; box-shadow: none; }
.mini.stack i, .mini.stack b { position: absolute; left: 0; right: 0; top: 0; bottom: 0; border-radius: 6px; background: var(--surface); border: 1px solid var(--line-strong); }
.mini.stack i:nth-child(1) { transform: translateY(6px) scale(.9); opacity: .7; }
.mini.stack i:nth-child(2) { transform: translateY(3px) scale(.95); }
.mini.stack b { box-shadow: var(--shadow-1); }
.mini .cloud { position: absolute; top: -7px; left: 6px; height: 14px; padding: 0 4px; display: inline-flex; align-items: center; border-radius: 7px; background: var(--surface); border: 1px solid var(--line-strong); color: var(--muted); }
.mini.proposed { border: 1.5px dashed var(--info-dot); background: var(--accent-soft); }
.zonepills { gap: 4px; }
.zp { font-size: 8.5px; font-weight: 650; letter-spacing: .06em; text-transform: uppercase; padding: 1px 6px; border-radius: 999px; }
.zp.active { color: var(--ok); background: var(--ok-bg); }
.zp.standby { color: var(--accent); background: var(--accent-soft); }
.zp.witness { color: var(--edge-quorum); background: #f0e9fb; }
.band { display: inline-block; width: 38px; height: 0; border-top: 4px solid; border-radius: 2px; }
.band.flow { border-color: var(--flow-band); } .band.lost { border-color: var(--flow-lost-band); border-top-style: dashed; }
.band.missed { border-color: var(--flow-missed-band); border-top-style: dashed; } .band.detour { border-color: var(--flow-detour-band); border-top-style: dashed; }
.pt { width: 8px; height: 8px; border-radius: 50%; margin-left: -28px; display: inline-block; }
.pt.flow { background: var(--flow); } .pt.lost { background: var(--flow-lost); } .pt.missed { background: var(--flow-missed); } .pt.detour { background: var(--flow-detour); }
.tagx { font-size: 10px; font-weight: 700; padding: 0 7px; height: 17px; line-height: 17px; border-radius: 9px; border: 1px solid var(--line-strong); background: var(--surface); color: var(--text-2); }
.tagx.up { background: var(--risk-bg); border-color: var(--risk-line); color: var(--risk); }
.tagx.st { border: 0; color: #fff; }
.tagx.st.down { background: var(--crit); } .tagx.st.wait { background: var(--risk); }
.pop-enter-active { transition: opacity var(--dur-base) var(--ease-out), transform var(--dur-base) var(--ease-out); }
.pop-leave-active { transition: opacity var(--dur-instant) var(--ease-in); }
.pop-enter-from { opacity: 0; transform: translateY(calc(var(--move) * -.75)); }
.pop-leave-to { opacity: 0; }
</style>
