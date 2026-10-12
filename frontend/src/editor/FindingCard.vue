<script setup lang="ts">
import { computed, ref } from 'vue'
import { ChevronDown, ChevronRight, Sparkles } from 'lucide-vue-next'
import type { Finding } from '@/api/types'
import { lt, t } from '@/i18n'
import { findingText, findingTitle, fixLabel, statusLabel, statusTagClass } from '@/lib/findings'
import { useAi } from '@/stores/ai'
import { useApp } from '@/stores/app'
import { useEditor } from '@/stores/editor'
import AcceptRiskModal from './AcceptRiskModal.vue'

const props = defineProps<{ finding: Finding }>()
const app = useApp()
const editor = useEditor()
const ai = useAi()

const rule = computed(() => app.rules.get(props.finding.ruleId))
const fixes = computed(() => props.finding.fixes.filter((f) => f.ops.length))
const hasSubjects = computed(() => props.finding.subjects.nodes.length + props.finding.subjects.edges.length + props.finding.subjects.zones.length > 0)
const canAccept = computed(() => app.canEdit && !editor.readonly && (props.finding.status === 'risk' || props.finding.status === 'critical') && !props.finding.accepted)
const canFix = computed(() => app.canEdit && !editor.readonly)
const canAi = computed(() => canFix.value && ai.enabled && !ai.busy)
const accepting = ref(false)
const why = ref(false)
const busy = ref(false)

async function fix(i: number) {
  busy.value = true
  try { await editor.applyFix(props.finding, props.finding.fixes.indexOf(fixes.value[i])) } finally { busy.value = false }
}

async function revoke() {
  busy.value = true
  try { await editor.revokeWaiver(props.finding.id) } finally { busy.value = false }
}
</script>

<template>
  <article class="finding" :class="[finding.status, finding.accepted && 'accepted']">
    <div class="top">
      <span class="tag" :class="statusTagClass(finding.status)">{{ finding.accepted ? t('checks.accepted') : statusLabel(finding.status) }}</span>
      <span class="grow" />
      <span class="rid mono">{{ finding.ruleId }}</span>
    </div>
    <h4>{{ findingTitle(rule, finding) }}</h4>
    <p class="text">{{ findingText(rule, finding) }}</p>
    <p v-if="finding.accepted" class="reason">{{ t('checks.accepted.by', { reason: finding.accepted.reason }) }}</p>

    <div v-if="why && rule" class="why">
      <p v-if="rule.rationale">{{ lt(rule.rationale) }}</p>
      <p v-if="rule.remediation"><b>{{ t('checks.remedy') }}</b> {{ lt(rule.remediation) }}</p>
      <p v-if="rule.references.length" class="faint">{{ rule.references.join(' · ') }}</p>
    </div>

    <div class="actions">
      <template v-if="!finding.accepted">
        <button v-for="(f, i) in fixes.slice(0, 2)" :key="f.id" class="btn btn-sm" :class="i === 0 ? 'btn-primary' : ''" :disabled="!canFix || busy" @click="fix(i)">{{ t('checks.fix', { label: fixLabel(rule, f, finding) }) }}</button>
        <button v-if="canAccept" class="btn btn-sm" @click="accepting = true">{{ t('checks.accept') }}</button>
        <button v-if="finding.scenario && finding.scenario.type !== 'none'" class="btn btn-sm" @click="editor.runScenario(finding.scenario!)">{{ t('checks.replay') }}</button>
        <button v-if="hasSubjects" class="btn btn-sm" @click="editor.showSubjects(finding)">{{ t('checks.show') }}</button>
        <button v-if="canAi && finding.status !== 'no_data'" class="btn btn-sm btn-ghost" :title="t('checks.ai.fix')" @click="ai.submit('fix', '', finding.id)"><Sparkles :size="13" />{{ t('checks.ai.fix.short') }}</button>
      </template>
      <button v-else-if="app.canEdit && !editor.readonly" class="btn btn-sm" :disabled="busy" @click="revoke">{{ t('checks.revoke') }}</button>
      <span class="grow" />
      <button v-if="rule?.rationale" class="why-btn" @click="why = !why"><component :is="why ? ChevronDown : ChevronRight" :size="13" />{{ t('checks.why') }}</button>
    </div>
    <AcceptRiskModal v-if="accepting" :finding="finding" @close="accepting = false" />
  </article>
</template>

<style scoped>
.finding { background: var(--surface); border: 1px solid var(--line-strong); border-radius: var(--radius-lg); padding: 14px 16px 14px; }
.finding.accepted { background: var(--surface-2); }
.top { display: flex; align-items: center; margin-bottom: 8px; }
.top .tag { height: 24px; padding: 0 11px; border-radius: 12px; font-size: 11px; }
.tag.tag-nodata { background: var(--nodata-bg); border: 1px solid var(--nodata-line); }
.rid { font-size: 12px; color: var(--faint); letter-spacing: .02em; }
h4 { font-size: 14.5px; margin: 0 0 4px; }
.text { margin: 0 0 10px; color: var(--text-2); font-size: 13.5px; line-height: 1.5; }
.reason { margin: -4px 0 10px; font-size: 12.5px; color: var(--muted); font-style: italic; }
.actions .btn { height: auto; min-height: 28px; padding-top: 4px; padding-bottom: 4px; white-space: normal; text-align: left; line-height: 1.3; }
.actions { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
.why { margin: 0 0 10px; padding: 10px 12px; background: var(--surface-2); border-radius: var(--radius-sm); font-size: 12.5px; color: var(--text-2); }
.why p { margin: 0 0 6px; }
.why p:last-child { margin: 0; }
.why-btn { display: inline-flex; align-items: center; gap: 3px; border: 0; background: transparent; color: var(--muted); font-size: 12px; padding: 2px 4px; border-radius: 4px; }
.why-btn:hover { color: var(--text); background: var(--surface-sunken); }
</style>
