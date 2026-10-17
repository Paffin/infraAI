<script setup lang="ts">
// The gallery of reference architectures in the "New project" dialog: a search that looks at names, descriptions and tags, filters by the tags the templates carry
// (the most frequent ones; how many is `editor.galleryTagsShown`), and a card per architecture with what it promises: the size, the availability target and the
// recovery goals. The first card is always the blank sheet. Every card is a radio button, so the keyboard works as it does for any group of choices.
import { computed, ref } from 'vue'
import { Search } from 'lucide-vue-next'
import type { TemplateMeta } from '@/api/types'
import { has, lt, t } from '@/i18n'
import { fmtDuration } from '@/lib/format'
import { useApp } from '@/stores/app'

const props = defineProps<{ templates: TemplateMeta[]; modelValue: string }>()
const emit = defineEmits<{ 'update:modelValue': [id: string] }>()
const app = useApp()

const query = ref('')
const tag = ref('')

const tagLabel = (id: string) => (has('tpl.tag.' + id) ? t('tpl.tag.' + id) : id)
/** The tags by how many architectures carry them, the most frequent first. */
const tagCounts = computed(() => {
  const n = new Map<string, number>()
  for (const tpl of props.templates) for (const x of tpl.tags ?? []) n.set(x, (n.get(x) ?? 0) + 1)
  return [...n.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
})
const filters = computed(() => tagCounts.value.slice(0, app.editorCfg.galleryTagsShown))

const haystack = (tpl: TemplateMeta) => [lt(tpl.name), lt(tpl.description), tpl.badge ?? '', tpl.id, ...(tpl.tags ?? []).flatMap((x) => [x, tagLabel(x)])].join(' ').toLowerCase()
const shown = computed(() => {
  const words = query.value.toLowerCase().split(/\s+/).filter(Boolean)
  return props.templates.filter((tpl) => (!tag.value || (tpl.tags ?? []).includes(tag.value)) && words.every((w) => haystack(tpl).includes(w)))
})
const filtering = computed(() => !!tag.value || !!query.value.trim())

function facts(tpl: TemplateMeta): string[] {
  const r = tpl.requirements
  const out = [t('gallery.nodes', { n: tpl.nodeCount })]
  if (typeof r.availabilitySlo === 'number') out.push(`SLO ${r.availabilitySlo}%`)
  if (typeof r.rpoSeconds === 'number') out.push(`RPO ${r.rpoSeconds === 0 ? '0' : fmtDuration(r.rpoSeconds)}`)
  if (typeof r.rtoSeconds === 'number') out.push(`RTO ${fmtDuration(r.rtoSeconds)}`)
  return out
}
function toggle(id: string) {
  tag.value = tag.value === id ? '' : id
}
</script>

<template>
  <div class="gallery" data-testid="gallery">
    <div class="bar">
      <label class="find">
        <Search :size="14" aria-hidden="true" />
        <input v-model="query" class="input" type="search" :placeholder="t('gallery.search')" :aria-label="t('gallery.search')" data-testid="gallery-search" />
      </label>
      <span class="muted count" aria-live="polite" data-testid="gallery-count">{{ t('gallery.count', { n: shown.length }) }}</span>
    </div>
    <div v-if="filters.length" class="chips" role="group" :aria-label="t('gallery.tags')">
      <button type="button" class="chip" :class="{ on: !tag }" :aria-pressed="!tag" @click="tag = ''">{{ t('gallery.all') }}</button>
      <button v-for="[id, n] in filters" :key="id" type="button" class="chip" :class="{ on: tag === id }" :aria-pressed="tag === id" :data-tag="id" @click="toggle(id)">
        {{ tagLabel(id) }}<span class="n">{{ n }}</span>
      </button>
    </div>
    <div class="cards" role="radiogroup" :aria-label="t('projects.create.from')">
      <label class="tpl" :class="{ on: !modelValue }" data-testid="tpl-blank">
        <input type="radio" name="template" value="" :checked="!modelValue" @change="emit('update:modelValue', '')" />
        <b>{{ t('projects.create.blank') }}</b>
        <span class="muted desc">{{ t(app.boot?.ai.enabled ? 'projects.create.blank.hint' : 'projects.create.blank.hint.manual') }}</span>
      </label>
      <label v-for="tpl in shown" :key="tpl.id" class="tpl" :class="{ on: modelValue === tpl.id }" :data-template="tpl.id">
        <input type="radio" name="template" :value="tpl.id" :checked="modelValue === tpl.id" @change="emit('update:modelValue', tpl.id)" />
        <span class="row"><b class="grow">{{ lt(tpl.name) }}</b><span v-if="tpl.badge" class="tag tag-accent tag-mono">{{ tpl.badge }}</span></span>
        <span class="muted desc">{{ lt(tpl.description) }}</span>
        <span class="facts"><span v-for="f in facts(tpl)" :key="f">{{ f }}</span></span>
      </label>
      <p v-if="filtering && !shown.length" class="none muted" data-testid="gallery-none">{{ t('gallery.none') }}</p>
    </div>
  </div>
</template>

<style scoped>
.gallery { display: flex; flex-direction: column; gap: 10px; min-width: 0; }
.bar { display: flex; align-items: center; gap: 12px; }
.find { position: relative; flex: 1; }
.find svg { position: absolute; left: 10px; top: 11px; color: var(--faint); pointer-events: none; }
.find .input { padding-left: 32px; width: 100%; }
.count { font-size: 12.5px; flex: none; }
.chips { display: flex; flex-wrap: wrap; gap: 6px; }
.chip { display: inline-flex; align-items: center; gap: 6px; padding: 4px 10px; border: 1px solid var(--line-strong); border-radius: 999px; background: var(--surface); color: var(--text-2); font-size: 12.5px; }
.chip:hover { border-color: #b8c2d4; }
.chip.on { background: var(--accent-soft); border-color: var(--accent-line); color: var(--accent); font-weight: 600; }
.chip .n { font-size: 11px; color: var(--faint); font-variant-numeric: tabular-nums; }
.chip.on .n { color: inherit; opacity: .8; }
.cards { display: grid; grid-template-columns: repeat(auto-fill, minmax(250px, 1fr)); gap: 8px; max-height: min(46vh, 440px); overflow: auto; padding: 2px; margin: -2px; align-content: start; }
.tpl { display: flex; flex-direction: column; gap: 4px; border: 1px solid var(--line-strong); border-radius: var(--radius); padding: 10px 12px; cursor: pointer; font-size: 13px; min-width: 0; transition: border-color var(--dur-fast), background-color var(--dur-fast); }
.tpl:hover { border-color: #b8c2d4; }
.tpl.on { border-color: var(--accent); background: var(--accent-soft); }
.tpl:has(input:focus-visible) { outline: 2px solid var(--accent); outline-offset: 1px; }
.tpl input { position: absolute; opacity: 0; pointer-events: none; }
.row { display: flex; align-items: center; gap: 8px; }
.desc { display: -webkit-box; -webkit-line-clamp: 3; -webkit-box-orient: vertical; overflow: hidden; line-height: 1.4; }
.facts { display: flex; flex-wrap: wrap; gap: 4px 10px; margin-top: auto; padding-top: 4px; font-size: 11.5px; color: var(--muted); font-family: var(--mono); }
.none { grid-column: 1 / -1; margin: 8px 0; text-align: center; }
</style>
