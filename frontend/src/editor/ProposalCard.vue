<script setup lang="ts">
import { computed, ref } from 'vue'
import { ArrowRight, Eye, EyeOff, Sparkles } from 'lucide-vue-next'
import { t } from '@/i18n'
import { diffIsEmpty, diffLines } from '@/lib/diff'
import { useAi } from '@/stores/ai'
import { useEditor } from '@/stores/editor'

const ai = useAi()
const editor = useEditor()

const p = computed(() => ai.proposal)
const lines = computed(() => (p.value ? diffLines(p.value.preview.diff) : []))
const shownLines = computed(() => lines.value.slice(0, 14))
const before = computed(() => p.value?.preview.before)
const after = computed(() => p.value?.preview.after)
const delta = computed(() => (before.value && after.value ? Math.round(after.value.score - before.value.score) : 0))
const introduced = computed(() => p.value?.preview.introduced.length ?? 0)
const resolved = computed(() => p.value?.preview.resolved.length ?? 0)
const pending = computed(() => p.value?.status === 'pending' && !ai.proposalStale)
const previewing = computed(() => !!editor.preview)
const expanded = ref(false)
const baseEmpty = computed(() => editor.isEmpty)
const longText = computed(() => (p.value?.rationale.length ?? 0) > 330)

function togglePreview() {
  if (previewing.value) editor.setPreview(null)
  else if (p.value) void ai.openProposal(p.value.id)
}
</script>

<template>
  <section v-if="p" class="proposal">
    <div class="lead">
      <Sparkles :size="14" class="spark" />
      <p :class="{ clamp: longText && !expanded }"><b>{{ t('proposal.assistant') }}</b> · {{ p.rationale || (p.mode === 'replace' ? t('proposal.replace') : t('proposal.title')) }}</p>
    </div>

    <button v-if="longText" class="expand" @click="expanded = !expanded">{{ expanded ? t('proposal.collapse') : t('proposal.expand') }}</button>

    <div v-if="before && after" class="scores">
      <span class="muted">{{ t('proposal.score') }}</span>
      <template v-if="!baseEmpty"><b class="mono">{{ Math.round(before.score) }}</b><ArrowRight :size="13" class="muted" /></template>
      <b class="mono" :class="delta > 0 && !baseEmpty ? 'up' : delta < 0 && !baseEmpty ? 'down' : ''">{{ Math.round(after.score) }}</b>
      <span v-if="resolved" class="tag tag-ok">{{ t('proposal.resolved', { n: resolved }) }}</span>
      <span v-if="introduced" class="tag tag-risk">{{ t('proposal.introduced', { n: introduced }) }}</span>
    </div>
    <p v-if="introduced && delta < 0 && !baseEmpty" class="warn">{{ t('proposal.worse') }}</p>

    <div v-if="lines.length" class="diff mono">
      <div v-for="(l, i) in shownLines" :key="i" class="ln" :class="l.sign === '+' ? 'add' : l.sign === '-' ? 'del' : 'chg'"><span class="sg">{{ l.sign === '-' ? '−' : l.sign }}</span><span class="tx">{{ l.text }}</span></div>
      <div v-if="lines.length > shownLines.length" class="more">… {{ t('proposal.more', { n: lines.length - shownLines.length }) }}</div>
    </div>
    <p v-else-if="diffIsEmpty(p.preview.diff)" class="muted small">{{ t('diff.nothing') }}</p>

    <p v-if="ai.proposalStale || p.status === 'stale'" class="warn">{{ t('proposal.stale') }}</p>
    <p v-else-if="pending" class="muted small">{{ t('proposal.rollback') }}</p>

    <div class="actions">
      <template v-if="pending">
        <button class="btn btn-primary" :disabled="ai.proposalBusy" @click="ai.applyProposal()">{{ t('proposal.apply') }}</button>
        <button class="btn" @click="togglePreview"><component :is="previewing ? EyeOff : Eye" :size="14" />{{ previewing ? t('proposal.hidePreview') : t('proposal.preview') }}</button>
        <span class="grow" />
        <button class="btn btn-ghost" :disabled="ai.proposalBusy" @click="ai.rejectProposal()">{{ t('proposal.reject') }}</button>
      </template>
      <button v-else class="btn" @click="ai.closeProposal()">{{ t('common.close') }}</button>
    </div>
  </section>
</template>

<style scoped>
.proposal { background: var(--accent-soft); border: 1px solid var(--accent-line); border-radius: var(--radius-lg); padding: 14px 16px; display: flex; flex-direction: column; gap: 10px; }
.lead { display: flex; gap: 8px; align-items: flex-start; }
.lead p { margin: 0; font-size: 13.5px; line-height: 1.5; color: var(--text-2); }
.lead b { color: var(--accent); }
.clamp { display: -webkit-box; -webkit-line-clamp: 6; -webkit-box-orient: vertical; overflow: hidden; }
.expand { align-self: flex-start; border: 0; background: transparent; color: var(--accent); font-size: 12.5px; padding: 0; margin-top: -4px; }
.expand:hover { text-decoration: underline; }
.spark { color: var(--accent); margin-top: 3px; flex: none; }
.scores { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; font-size: 13px; }
.scores b { font-size: 15px; }
.up { color: var(--ok); } .down { color: var(--crit); }
.warn { margin: 0; font-size: 12.5px; color: var(--risk); }
.small { font-size: 12.5px; margin: 0; }
.diff { background: var(--surface); border: 1px solid var(--line); border-radius: var(--radius-sm); padding: 8px 10px; font-size: 12px; line-height: 1.55; max-height: 190px; overflow: auto; }
.ln { display: flex; gap: 8px; }
.sg { width: 10px; flex: none; }
.tx { overflow-wrap: anywhere; }
.add { color: var(--ok); } .del { color: var(--crit); } .chg { color: var(--risk); }
.more { color: var(--faint); }
.actions { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
</style>
