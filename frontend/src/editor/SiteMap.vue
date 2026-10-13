<script setup lang="ts">
import { computed } from 'vue'
import type { Architecture, PlacedSite } from '@/api/types'
import { lt, t } from '@/i18n'
import { buildSiteMap, ROLE_MARK, type MapChip } from '@/lib/placementMap'
import { useApp } from '@/stores/app'

const props = defineProps<{ architecture: Architecture; sites: PlacedSite[] }>()
const app = useApp()
const map = computed(() => buildSiteMap(props.architecture, props.sites, app.index!))
const kindName = (c: MapChip) => lt(app.index!.kinds.get(c.kind)?.name) || c.kind
const tip = (c: MapChip) => [kindName(c), c.role, c.count > 1 ? `×${c.count}` : ''].filter(Boolean).join(' · ')
const sitesCount = computed(() => map.value.domains.reduce((n, d) => n + d.sites.length, 0))
</script>

<template>
  <div class="map" :class="{ dense: sitesCount > 8 }">
    <div v-if="map.global.length" class="global">
      <span v-for="c in map.global" :key="c.key" class="chip-s" :style="{ '--c': c.color }" :title="tip(c)">{{ c.abbr }}</span>
      <span class="gl faint">{{ t('placement.map.global') }}</span>
    </div>
    <section v-for="d in map.domains" :key="d.geo" class="domain">
      <h5 class="mono">{{ d.geo }}<span>{{ d.sites.length }}</span></h5>
      <div class="sites">
        <article v-for="(s, i) in d.sites" :key="s.id" class="site" :class="s.role" :style="{ '--i': i }">
          <header>
            <b class="truncate">{{ s.name }}</b>
            <span v-if="s.role !== 'working'" class="role">{{ t('placement.role.' + s.role) }}</span>
          </header>
          <div v-if="s.chips.length" class="chips">
            <span v-for="c in s.chips" :key="c.key" class="chip-s" :class="{ w: c.role === 'witness' }" :style="{ '--c': c.color }" :title="tip(c)">
              {{ c.abbr }}<sup v-if="c.role && ROLE_MARK[c.role]">{{ ROLE_MARK[c.role] }}</sup><em v-if="c.count > 1">×{{ c.count }}</em>
            </span>
          </div>
          <div v-for="(r, ri) in s.racks" :key="r.id" class="rack">
            <span class="rn mono">{{ t('placement.rack', { n: ri + 1 }) }}</span>
            <span v-for="c in r.chips" :key="c.key" class="chip-s" :class="{ w: c.role === 'witness' }" :style="{ '--c': c.color }" :title="tip(c)">
              {{ c.abbr }}<sup v-if="c.role && ROLE_MARK[c.role]">{{ ROLE_MARK[c.role] }}</sup><em v-if="c.count > 1">×{{ c.count }}</em>
            </span>
          </div>
        </article>
      </div>
    </section>
  </div>
</template>

<style scoped>
.map { display: flex; flex-direction: column; gap: 12px; }
.global { display: flex; align-items: center; gap: 6px; padding: 8px 10px; border: 1px dashed var(--line-strong); border-radius: var(--radius); background: var(--surface-2); }
.gl { font-size: 11.5px; margin-left: 4px; }
.domain { border: 1px solid var(--line); border-radius: var(--radius-lg); padding: 8px 10px 10px; background: var(--surface); }
.domain h5 { margin: 0 0 8px; font-size: 11.5px; font-weight: 600; color: var(--muted); display: flex; align-items: center; gap: 8px; letter-spacing: .02em; }
.domain h5 span { font-family: var(--font); background: var(--surface-sunken); border-radius: 10px; padding: 0 7px; font-size: 11px; color: var(--faint); }
.sites { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 8px; }
.dense .sites { grid-template-columns: repeat(auto-fill, minmax(132px, 1fr)); }
.site { border: 1px solid var(--line-strong); border-radius: var(--radius); padding: 8px 9px 9px; background: var(--surface); display: flex; flex-direction: column; gap: 7px; min-width: 0; animation: site-in var(--dur-base) var(--ease-out) both; animation-delay: calc(var(--i, 0) * var(--stagger)); }
.site.witness, .site.backup { border-style: dashed; background: var(--surface-2); }
.site.standby { border-color: var(--risk-line); background: var(--risk-bg); }
.site header { display: flex; align-items: center; gap: 6px; font-size: 12.5px; }
.site header b { flex: 1; font-weight: 600; }
.role { font-size: 10.5px; padding: 1px 6px; border-radius: 9px; background: var(--surface-sunken); color: var(--muted); white-space: nowrap; }
.standby .role { background: #fff; color: var(--risk); }
.chips, .rack { display: flex; flex-wrap: wrap; gap: 4px; align-items: center; }
.rack { padding-top: 5px; border-top: 1px dashed var(--line); }
.rn { font-size: 10px; color: var(--faint); width: 100%; }
.chip-s { --c: #8b93a1; display: inline-flex; align-items: baseline; gap: 1px; height: 22px; padding: 0 6px; align-items: center; border-radius: 5px; font: 600 10.5px/1 var(--mono); letter-spacing: .02em; color: var(--c); background: color-mix(in srgb, var(--c) 13%, #fff); border: 1px solid color-mix(in srgb, var(--c) 45%, #fff); }
.chip-s.w { border-style: dashed; }
.chip-s sup { font-size: 8.5px; margin-left: 2px; opacity: .85; }
.chip-s em { font-style: normal; font-size: 9.5px; opacity: .75; margin-left: 2px; }
@keyframes site-in { from { opacity: 0; transform: translateY(var(--move)) scale(.97); } to { opacity: 1; transform: none; } }
</style>
