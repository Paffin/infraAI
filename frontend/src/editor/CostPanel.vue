<script setup lang="ts">
import { computed } from 'vue'
import { lt, t } from '@/i18n'
import { fmtMoney } from '@/lib/format'
import { useApp } from '@/stores/app'
import { useEditor } from '@/stores/editor'

const app = useApp()
const editor = useEditor()
const cost = computed(() => editor.report?.cost)
const budget = computed(() => cost.value?.budget ?? editor.arch.requirements.budgetMonthly ?? null)
const sym = computed(() => cost.value?.symbol ?? app.boot?.engine.symbol ?? '')
const pct = computed(() => (cost.value && budget.value ? (cost.value.total / budget.value) * 100 : null))
const over = computed(() => (cost.value && budget.value ? cost.value.total - budget.value : 0))

function bars(map: Record<string, number> | undefined, label: (k: string) => string) {
  const entries = Object.entries(map ?? {}).filter(([, v]) => v > 0).sort((a, b) => b[1] - a[1])
  const max = entries.length ? entries[0][1] : 1
  return entries.map(([k, v]) => ({ key: k, label: label(k), value: v, width: Math.max(2, (v / max) * 100) }))
}
const byCategory = computed(() => bars(cost.value?.byCategory, (k) => lt(app.boot?.catalog.categories.find((c) => c.id === k)?.name) || k))
const byZone = computed(() => bars(cost.value?.byZone, (k) => (k === '_global' ? t('cost.global') : editor.arch.zones.find((z) => z.id === k)?.name ?? k)))
const top = computed(() => [...(cost.value?.nodes ?? [])].filter((n) => n.amount > 0).sort((a, b) => b.amount - a.amount).slice(0, 8).map((n) => ({ ...n, name: editor.arch.nodes.find((x) => x.id === n.nodeId)?.name ?? n.nodeId })))
</script>

<template>
  <div class="cost">
    <h3>{{ t('cost.title') }}</h3>
    <div v-if="!cost" class="empty"><span class="spinner" /></div>
    <template v-else>
      <div class="total card card-pad">
        <div class="big mono">{{ fmtMoney(cost.total, sym) }}</div>
        <div class="muted">{{ t('cost.total') }}</div>
        <template v-if="budget">
          <div class="bar"><i :class="{ over: (pct ?? 0) > 100 }" :style="{ width: Math.min(100, pct ?? 0) + '%' }" /></div>
          <div class="row small"><span class="muted">{{ t('cost.budget', { v: fmtMoney(budget, sym) }) }}</span><span class="spacer" /><b :class="{ crit: over > 0 }">{{ over > 0 ? t('cost.over', { v: fmtMoney(over, sym) }) : t('cost.used', { pct: Math.round(pct ?? 0) }) }}</b></div>
        </template>
      </div>
      <p v-if="cost.incomplete" class="banner banner-warn">{{ t('cost.incomplete', { n: cost.incomplete }) }}</p>

      <section v-if="byCategory.length">
        <h5>{{ t('cost.byCategory') }}</h5>
        <div v-for="b in byCategory" :key="b.key" class="brow"><span class="bl truncate">{{ b.label }}</span><span class="bt"><i :style="{ width: b.width + '%' }" /></span><span class="bv mono">{{ fmtMoney(b.value, sym) }}</span></div>
      </section>
      <section v-if="byZone.length">
        <h5>{{ t('cost.byZone') }}</h5>
        <div v-for="b in byZone" :key="b.key" class="brow"><span class="bl truncate">{{ b.label }}</span><span class="bt"><i :style="{ width: b.width + '%' }" /></span><span class="bv mono">{{ fmtMoney(b.value, sym) }}</span></div>
      </section>
      <section v-if="top.length">
        <h5>{{ t('cost.topNodes') }}</h5>
        <div v-for="n in top" :key="n.nodeId" class="brow plain">
          <button class="link truncate bl" @click="editor.select({ type: 'node', id: n.nodeId })">{{ n.name }}</button><span class="bv mono">{{ fmtMoney(n.amount, sym) }}</span>
        </div>
      </section>
      <p class="muted note">{{ t('cost.estimate') }}</p>
    </template>
  </div>
</template>

<style scoped>
.cost { padding: 16px 20px 28px; display: flex; flex-direction: column; gap: 14px; overflow: auto; }
h3 { font-size: 16px; }
h5 { margin: 0 0 8px; font-size: 11px; letter-spacing: .09em; text-transform: uppercase; color: #6e7fa8; font-weight: 600; }
.big { font-size: 26px; font-weight: 600; letter-spacing: -0.02em; }
.bar { height: 8px; background: var(--surface-sunken); border-radius: 4px; overflow: hidden; margin: 12px 0 8px; }
.bar i { display: block; height: 100%; background: var(--accent); border-radius: 4px; }
.bar i.over { background: var(--crit); }
.small { font-size: 12.5px; }
.crit { color: var(--crit); }
.brow { display: grid; grid-template-columns: minmax(80px, 1fr) 1.1fr auto; align-items: center; gap: 10px; font-size: 12.5px; padding: 3px 0; }
.brow.plain { grid-template-columns: 1fr auto; }
.bt { height: 8px; background: var(--surface-sunken); border-radius: 4px; overflow: hidden; }
.bt i { display: block; height: 100%; background: var(--info-dot); border-radius: 4px; }
.bv { color: var(--text-2); text-align: right; white-space: nowrap; }
.link { border: 0; background: transparent; padding: 0; text-align: left; color: var(--text); }
.link:hover { color: var(--accent); }
.note { font-size: 12px; margin: 0; }
</style>
