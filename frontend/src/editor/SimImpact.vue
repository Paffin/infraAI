<script setup lang="ts">
// Who the failure hits, in the picture on screen: the nodes that do not serve, the ones that serve with losses, the ones that took over.
// A click takes to the card on the diagram (and opens it in the inspector).
import { computed, ref } from 'vue'
import { t } from '@/i18n'
import { shortZone } from '@/lib/sim'
import type { Hit, Impact, Tier } from '@/lib/simImpact'
import { useApp } from '@/stores/app'
import { useEditor } from '@/stores/editor'

const props = defineProps<{ impact: Impact }>()
const app = useApp()
const editor = useEditor()
/** How many nodes of a group are listed until the list is opened (`editor.impactShown`). */
const limit = computed(() => app.editorCfg.impactShown)
const expanded = ref<Record<Tier, boolean>>({ out: false, hurt: false, took: false })

const groups = computed(() => {
  const all: { tier: Tier; hits: Hit[] }[] = [{ tier: 'out', hits: props.impact.out }, { tier: 'hurt', hits: props.impact.hurt }, { tier: 'took', hits: props.impact.took }]
  return all.filter((g) => g.hits.length).map((g) => ({ ...g, label: t('simimpact.' + g.tier), shown: expanded.value[g.tier] ? g.hits : g.hits.slice(0, limit.value) }))
})

function go(h: Hit) {
  editor.select({ type: 'node', id: h.id })
  editor.revealElements([h.id])
}
</script>

<template>
  <section class="impact" :aria-label="t('simimpact.title')" data-testid="sim-impact">
    <h3>{{ t('simimpact.title') }}</h3>
    <p v-if="!groups.length" class="none">{{ t('simimpact.none') }}</p>
    <div v-for="g in groups" :key="g.tier" class="grp" :class="g.tier" :data-tier="g.tier">
      <h4><i class="dot" aria-hidden="true" />{{ g.label }}<b>{{ g.hits.length }}</b></h4>
      <TransitionGroup name="list" tag="ul">
        <li v-for="h in g.shown" :key="h.id">
          <button class="hit" :class="h.state" :title="t('simimpact.go')" data-testid="sim-hit" @click="go(h)">
            <span class="n">{{ h.name }}</span>
            <span v-if="h.zone" class="z">{{ shortZone(h.zone) }}</span>
            <span class="s">{{ t('node.state.' + h.state) }}</span>
          </button>
        </li>
      </TransitionGroup>
      <button v-if="g.hits.length > limit" class="more" :aria-expanded="expanded[g.tier]" @click="expanded[g.tier] = !expanded[g.tier]">
        {{ expanded[g.tier] ? t('simimpact.less') : t('simimpact.more', { n: g.hits.length - limit }) }}
      </button>
    </div>
  </section>
</template>

<style scoped>
.impact { margin: 4px 0 2px; }
h3 { margin: 12px 0 6px; font-size: 10.5px; letter-spacing: .09em; text-transform: uppercase; color: var(--label); font-weight: 600; }
.none { margin: 0; font-size: 12.5px; color: var(--muted); }
.grp { margin: 0 0 8px; }
h4 { display: flex; align-items: center; gap: 7px; margin: 0 0 4px; font-size: 12px; font-weight: 600; color: var(--text-2); }
h4 b { font-weight: 600; font-variant-numeric: tabular-nums; color: var(--muted); }
.dot { width: 8px; height: 8px; border-radius: 50%; background: var(--crit); flex: none; }
.hurt .dot { background: var(--risk-dot); }
.took .dot { background: var(--ok-dot); }
ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 2px; }
.hit { display: flex; align-items: baseline; gap: 8px; width: 100%; padding: 4px 8px; border: 0; border-radius: var(--radius-sm); background: transparent; text-align: left; font-size: 12.5px; color: var(--text); transition: background-color var(--dur-fast); }
.hit:hover { background: var(--surface-sunken); }
.hit:focus-visible { outline: 2px solid var(--accent-line); outline-offset: -2px; }
.n { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 550; }
.z { flex: none; max-width: 40%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 11.5px; color: var(--muted); }
.s { margin-left: auto; flex: none; font-size: 11px; font-weight: 600; padding: 0 7px; border-radius: 9px; background: var(--crit-bg); color: var(--crit); white-space: nowrap; }
.hurt .s { background: var(--risk-bg); color: var(--risk); }
.took .s { background: var(--ok-bg); color: var(--ok); }
.more { margin: 2px 0 0 8px; padding: 2px 6px; border: 0; background: transparent; color: var(--accent); font-size: 12px; border-radius: var(--radius-sm); }
.more:hover { background: var(--accent-soft); }
</style>
