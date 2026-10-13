<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { CornerDownLeft, Search } from 'lucide-vue-next'
import { t } from '@/i18n'
import { rank } from '@/lib/fuzzy'
import { useApp } from '@/stores/app'
import { useUi } from '@/stores/ui'
import { GROUP_ORDER, usePaletteCommands, type Command } from './paletteCommands'

const ui = useUi()
const app = useApp()
const commands = usePaletteCommands()
const q = ref('')
const active = ref(0)
const input = ref<HTMLInputElement | null>(null)
const list = ref<HTMLElement | null>(null)

/** Best matches for the query; with an empty query the commands marked as suggestions. */
const results = computed<Command[]>(() => {
  const query = q.value.trim()
  const all = commands.value
  if (!query) return all.filter((c) => c.suggest)
  return rank(all, query, (c) => [c.title, c.hint ?? '', ...(c.keywords ?? [])]).slice(0, app.editorCfg.paletteMaxResults)
})

/** Sections in a fixed order (elements first, then what to do); inside a section the best match comes first. */
const sections = computed(() => GROUP_ORDER.map((g) => ({ group: g, items: results.value.filter((c) => c.group === g) })).filter((s) => s.items.length))
const flat = computed(() => sections.value.flatMap((s) => s.items))
const indexOf = (c: Command) => flat.value.indexOf(c)

watch(q, () => { active.value = 0 })
watch(flat, (f) => { if (active.value >= f.length) active.value = Math.max(0, f.length - 1) })

function close() { ui.palette = false }

function run(c: Command | undefined) {
  if (!c) return
  close()
  c.run()
}

function scrollToActive() {
  void nextTick(() => list.value?.querySelector<HTMLElement>('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest' }))
}

function onKey(e: KeyboardEvent) {
  const n = flat.value.length
  if (e.key === 'ArrowDown') { e.preventDefault(); active.value = n ? (active.value + 1) % n : 0; scrollToActive() }
  else if (e.key === 'ArrowUp') { e.preventDefault(); active.value = n ? (active.value - 1 + n) % n : 0; scrollToActive() }
  else if (e.key === 'Home' && !q.value) { e.preventDefault(); active.value = 0; scrollToActive() }
  else if (e.key === 'End' && !q.value) { e.preventDefault(); active.value = Math.max(0, n - 1); scrollToActive() }
  else if (e.key === 'Enter') { e.preventDefault(); run(flat.value[active.value]) }
  else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); close() }
  else if (e.key === 'Tab') { e.preventDefault() } // the dialog is one control: keep the focus in the input
}

onMounted(() => input.value?.focus())
</script>

<template>
  <Teleport to="body">
    <div class="modal-backdrop pal-backdrop" @mousedown.self="close">
      <div class="palette" role="dialog" aria-modal="true" :aria-label="t('palette.title')">
        <div class="pbar">
          <Search :size="17" class="mag" />
          <input
            ref="input" v-model="q" class="q" type="text" autocomplete="off" spellcheck="false" role="combobox" aria-expanded="true" aria-controls="pal-list"
            :aria-activedescendant="flat[active] ? 'pal-' + flat[active].id : undefined" :placeholder="t('palette.placeholder')" @keydown="onKey"
          />
          <kbd class="kbd">Esc</kbd>
        </div>

        <div id="pal-list" ref="list" class="results" role="listbox">
          <template v-for="s in sections" :key="s.group">
            <div class="gh">{{ t('palette.group.' + s.group) }}</div>
            <button
              v-for="c in s.items" :id="'pal-' + c.id" :key="c.id" class="item" :class="{ on: indexOf(c) === active }" role="option" :aria-selected="indexOf(c) === active"
              @mousemove="active = indexOf(c)" @click="run(c)"
            >
              <span v-if="c.badge" class="badge-abbr tile" :style="{ background: c.badge.color }">{{ c.badge.text }}</span>
              <span v-else class="tile ico" :style="{ color: c.color }"><component :is="c.icon" v-if="c.icon" :size="16" /></span>
              <span class="txt">
                <span class="ti truncate">{{ c.title }}</span>
                <span v-if="c.hint" class="hi truncate">{{ c.hint }}</span>
              </span>
              <span v-if="c.dot" class="dot" :style="{ background: c.dot }" />
              <kbd v-if="c.shortcut" class="kbd">{{ c.shortcut }}</kbd>
            </button>
          </template>
          <div v-if="!flat.length" class="none">{{ t('palette.empty', { q }) }}</div>
        </div>

        <div class="foot">
          <span><kbd class="kbd">↑</kbd><kbd class="kbd">↓</kbd> {{ t('palette.nav.move') }}</span>
          <span><kbd class="kbd"><CornerDownLeft :size="10" /></kbd> {{ t('palette.nav.run') }}</span>
          <span class="grow" />
          <span v-if="!q" class="faint tip truncate">{{ t('palette.tip') }}</span>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.pal-backdrop { align-items: flex-start; padding-top: 11vh; z-index: 110; }
.palette { width: min(640px, 100%); max-height: min(560px, 78vh); display: flex; flex-direction: column; background: var(--surface); border-radius: var(--radius-lg); box-shadow: var(--shadow-3); overflow: hidden; }
.pbar { display: flex; flex-direction: row; align-items: center; gap: 10px; padding: 0 16px; height: 54px; border-bottom: 1px solid var(--line); flex: none; }
.mag { color: var(--faint); flex: none; }
.q { flex: 1; min-width: 0; height: 100%; border: 0; outline: none; background: transparent; font-size: 15.5px; color: var(--text); }
.q::placeholder { color: var(--faint); }
.results { overflow: auto; padding: 6px 8px 8px; flex: 1; min-height: 80px; }
.gh { padding: 10px 10px 4px; font-size: 10.5px; letter-spacing: .09em; text-transform: uppercase; color: var(--label); font-weight: 600; }
.item { display: flex; align-items: center; gap: 12px; width: 100%; min-height: 44px; padding: 6px 10px; border: 0; border-radius: var(--radius); background: transparent; text-align: left; color: var(--text); }
.item.on { background: var(--accent-soft); }
.tile { width: 30px; height: 30px; border-radius: 7px; flex: none; font-size: 9.5px; }
.tile.ico { display: inline-flex; align-items: center; justify-content: center; background: var(--surface-sunken); color: var(--text-2); }
.txt { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 1px; }
.ti { font-size: 14px; font-weight: 550; }
.hi { font-size: 12px; color: var(--muted); }
.dot { width: 9px; height: 9px; border-radius: 50%; flex: none; }
.none { padding: 28px 12px; text-align: center; color: var(--muted); }
.foot span { white-space: nowrap; }
.foot { display: flex; align-items: center; gap: 16px; padding: 9px 16px; border-top: 1px solid var(--line); background: var(--surface-2); font-size: 12px; color: var(--muted); flex: none; }
.foot .kbd { margin-right: 3px; }
.foot .tip { flex: 0 1 auto; min-width: 0; }
.foot .kbd svg { vertical-align: -1px; }
@media (max-width: 640px) { .pal-backdrop { padding: 12px; padding-top: 8vh; } }
</style>
