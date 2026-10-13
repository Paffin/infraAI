<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { ChevronDown, ChevronRight, PanelLeftClose, Search } from 'lucide-vue-next'
import * as api from '@/api/endpoints'
import type { KindDef, TemplateMeta, Architecture, VariantDef } from '@/api/types'
import KindChip from '@/components/KindChip.vue'
import Modal from '@/components/Modal.vue'
import { lt, t } from '@/i18n'
import { functionGroups } from '@/lib/catalog'
import { iconFor } from '@/lib/icons'
import { errorMessage } from '@/lib/format'
import { useApp } from '@/stores/app'
import { useEditor } from '@/stores/editor'
import { useUi } from '@/stores/ui'
import { useToasts } from '@/stores/toasts'

const app = useApp()
const editor = useEditor()
const toasts = useToasts()
const ui = useUi()

const q = ref('')
const input = ref<HTMLInputElement | null>(null)
const pending = ref<{ meta: TemplateMeta; arch: Architecture } | null>(null)

const query = computed(() => q.value.trim().toLowerCase())
const matches = (...parts: (string | null | undefined)[]) => !query.value || parts.some((p) => p && p.toLowerCase().includes(query.value))

const templates = computed(() => [...(app.boot?.templates ?? [])].sort((a, b) => a.order - b.order).filter((x) => matches(lt(x.name), lt(x.description), x.id)))
/** A product tile: the product, and the variant that made the search find it (clicking adds the node with that variant). */
interface Tile { kind: KindDef; variant?: VariantDef }

/** Does the query hit the product itself, or one of its variants? */
function tileOf(k: KindDef, fnName: string): Tile | null {
  if (!query.value) return { kind: k }
  if (matches(lt(k.name), k.abbr, k.id, lt(k.description), fnName)) return { kind: k }
  const v = (k.variants ?? []).find((x) => matches(lt(x.name), x.id, lt(x.description)))
  return v ? { kind: k, variant: v } : null
}

const everything = computed(() => {
  const cat = app.boot?.catalog
  if (!cat) return []
  return [...cat.categories].sort((a, b) => a.order - b.order).map((c) => {
    const names = new Map((c.functions ?? []).map((f) => [f.id, lt(f.name)]))
    const fns = functionGroups(cat, c.id).map((g) => {
      const tiles = g.kinds.map((k) => tileOf(k, names.get(g.id) ?? '')).filter((x): x is Tile => !!x)
      return { key: `${c.id}/${g.id}`, id: g.id, name: g.id ? lt(g.name) : '', tiles }
    }).filter((g) => g.tiles.length)
    return { id: c.id, name: lt(c.name), fns, count: fns.reduce((n, g) => n + g.tiles.length, 0) }
  }).filter((c) => c.fns.length)
})
const found = computed(() => everything.value.reduce((n, c) => n + c.count, 0))
/** A search that hits hundreds of products shows the first of them (`editor.librarySearchShown`): the rest is behind "Show more". */
const showAll = ref(false)
watch(query, () => { showAll.value = false })
const categories = computed(() => {
  const all = everything.value
  if (!query.value || showAll.value) return all
  let room = app.editorCfg.librarySearchShown
  return all.map((c) => {
    const fns = c.fns.map((g) => { const tiles = g.tiles.slice(0, Math.max(0, room)); room -= tiles.length; return { ...g, tiles } }).filter((g) => g.tiles.length)
    return { ...c, fns }
  }).filter((c) => c.fns.length)
})
const shownCount = computed(() => categories.value.reduce((n, c) => n + c.fns.reduce((m, g) => m + g.tiles.length, 0), 0))
/** Tiles of the functions that have one product only. */
const singles = (c: { fns: { tiles: Tile[] }[] }) => c.fns.filter((g) => g.tiles.length === 1).flatMap((g) => g.tiles)

// A category folds as a whole: the library holds hundreds of products, and a person looks for one corner of it at a time.
const CAT_KEY = 'ia_lib_cat'
/** The reference architectures fold the same way: a design under way does not need them every time the list is scrolled. */
const TEMPLATES = 'templates'
function loadClosed(): Record<string, boolean> {
  try { return JSON.parse(localStorage.getItem(CAT_KEY) ?? '{}') } catch { return {} }
}
const closed = ref<Record<string, boolean>>(loadClosed())
const catOpen = (id: string) => !!query.value || !closed.value[id]
function toggleCat(id: string) {
  closed.value = { ...closed.value, [id]: catOpen(id) }
  try { localStorage.setItem(CAT_KEY, JSON.stringify(closed.value)) } catch { /* private mode */ }
}

// Groups with many products start folded: a hundred buttons in one list help nobody. A search opens everything it finds.
const OPEN_KEY = 'ia_lib_open'
function loadOpen(): Record<string, boolean> {
  try { return JSON.parse(localStorage.getItem(OPEN_KEY) ?? '{}') } catch { return {} }
}
const openState = ref<Record<string, boolean>>(loadOpen())
const SMALL_GROUP = 3
/** How many products of a folded group stay visible. */
const PEEK = 2
const isOpen = (key: string, size: number) => !!query.value || (openState.value[key] ?? size <= SMALL_GROUP)
function toggle(key: string, size: number) {
  openState.value = { ...openState.value, [key]: !isOpen(key, size) }
  try { localStorage.setItem(OPEN_KEY, JSON.stringify(openState.value)) } catch { /* private mode */ }
}
function setAll(open: boolean) {
  const next: Record<string, boolean> = {}
  for (const c of everything.value) for (const g of c.fns) next[g.key] = open
  openState.value = next
  try { localStorage.setItem(OPEN_KEY, JSON.stringify(next)) } catch { /* private mode */ }
  closed.value = {}
  try { localStorage.setItem(CAT_KEY, '{}') } catch { /* private mode */ }
}
const tileTitle = (k: KindDef) => [lt(k.description) || t('library.hint'), k.variants?.length ? `${t('product.variants', { n: k.variants.length })}: ${k.variants.map((v) => lt(v.name)).join(' · ')}` : ''].filter(Boolean).join('\n')
const zoneTypes = computed(() => (app.boot?.catalog.zoneTypes ?? []).filter((z) => matches(lt(z.name), z.id)))

function dragKind(e: DragEvent, type: 'kind' | 'zone', id: string, variant?: string) {
  e.dataTransfer?.setData('application/x-infra-item', JSON.stringify({ type, id, variant }))
  if (e.dataTransfer) e.dataTransfer.effectAllowed = 'copy'
}

function addKind(tile: Tile) {
  if (!editor.readonly) editor.addNode(tile.kind.id, undefined, undefined, tile.variant ? { variant: tile.variant.id } : undefined)
}

async function pickTemplate(meta: TemplateMeta) {
  if (editor.readonly) return
  try {
    const tpl = await api.engine.template(meta.id)
    if (editor.isEmpty) editor.addTemplate(tpl.architecture, 'replace')
    else pending.value = { meta, arch: tpl.architecture }
  } catch (e) {
    toasts.error(errorMessage(e))
  }
}

function applyTemplate(mode: 'replace' | 'add') {
  if (pending.value) editor.addTemplate(pending.value.arch, mode)
  pending.value = null
}

defineExpose({ focus: () => { ui.libraryOpen = true; setTimeout(() => input.value?.focus(), 30) } })
</script>

<template>
  <aside v-show="ui.libraryOpen" class="library" data-tour="library" :aria-label="t('library.title')">
    <div class="search">
      <Search :size="15" />
      <input ref="input" v-model="q" class="input" :placeholder="t('library.search') + ' ( / )'" @keydown.esc="q = ''; input?.blur()" />
      <button class="collapse" :title="t('library.collapse')" :aria-label="t('library.collapse')" @click="ui.libraryOpen = false"><PanelLeftClose :size="16" /></button>
    </div>

    <div v-if="!query && categories.length" class="bar">
      <button class="lnk" @click="setAll(true)">{{ t('library.expandAll') }}</button>
      <button class="lnk" @click="setAll(false)">{{ t('library.collapseAll') }}</button>
    </div>
    <div v-else-if="query" class="bar"><span class="faint">{{ t('library.found', { n: found + templates.length }) }}</span></div>

    <div class="scroll">
      <section v-if="templates.length">
        <h2>
          <button class="ch" :aria-expanded="catOpen(TEMPLATES)" data-testid="lib-cat-templates" @click="toggleCat(TEMPLATES)">
            <component :is="catOpen(TEMPLATES) ? ChevronDown : ChevronRight" :size="13" />
            <span>{{ t('library.templates') }}</span><span class="found">{{ templates.length }}</span>
          </button>
        </h2>
        <template v-if="catOpen(TEMPLATES)">
          <button v-for="tpl in templates" :key="tpl.id" class="tpl" :disabled="editor.readonly" :title="lt(tpl.description)" @click="pickTemplate(tpl)">
            <span class="grow tname">{{ lt(tpl.name) }}</span>
            <span v-if="tpl.badge" class="badge">{{ tpl.badge }} ✓</span>
          </button>
        </template>
      </section>

      <section v-for="c in categories" :key="c.id">
        <h2>
          <button class="ch" :aria-expanded="catOpen(c.id)" :data-testid="'lib-cat-' + c.id" @click="toggleCat(c.id)">
            <component :is="catOpen(c.id) ? ChevronDown : ChevronRight" :size="13" />
            <span>{{ c.name }}</span><span class="found">{{ c.count }}</span>
          </button>
        </h2>
        <template v-if="catOpen(c.id)">
        <!-- products that are alone in their job need no heading: they share one grid -->
        <div v-if="singles(c).length" class="grid">
          <button
            v-for="x in singles(c)" :key="x.kind.id" class="kind" :data-kind="x.kind.id" :data-variant="x.variant?.id" :disabled="editor.readonly" draggable="true" :title="tileTitle(x.kind)"
            @dragstart="dragKind($event, 'kind', x.kind.id, x.variant?.id)" @click="addKind(x)"
          >
            <KindChip :kind="x.kind" :size="28" />
            <span class="two">
              <span class="nm">{{ lt(x.kind.name) }}</span>
              <span v-if="x.variant" class="sub">{{ lt(x.variant.name) }}</span>
            </span>
          </button>
        </div>
        <div v-for="g in c.fns.filter((x) => x.tiles.length > 1)" :key="g.key" class="fn">
          <button v-if="g.name" class="fh" :aria-expanded="isOpen(g.key, g.tiles.length)" @click="toggle(g.key, g.tiles.length)">
            <component :is="isOpen(g.key, g.tiles.length) ? ChevronDown : ChevronRight" :size="13" />
            <span class="grow truncate">{{ g.name }}</span>
            <span class="cnt">{{ g.tiles.length }}</span>
          </button>
          <!-- folded: the first products (the usual choices) stay at hand, the rest is one click away -->
          <div class="grid">
            <button
              v-for="x in (isOpen(g.key, g.tiles.length) ? g.tiles : g.tiles.slice(0, PEEK))" :key="x.kind.id" class="kind" :data-kind="x.kind.id" :data-variant="x.variant?.id" :disabled="editor.readonly" draggable="true" :title="tileTitle(x.kind)"
              @dragstart="dragKind($event, 'kind', x.kind.id, x.variant?.id)" @click="addKind(x)"
            >
              <KindChip :kind="x.kind" :size="28" />
              <span class="two">
                <span class="nm">{{ lt(x.kind.name) }}</span>
                <span v-if="x.variant" class="sub">{{ lt(x.variant.name) }}</span>
              </span>
            </button>
            <button v-if="!isOpen(g.key, g.tiles.length)" class="kind more-tile" @click="toggle(g.key, g.tiles.length)">{{ t('library.more', { n: g.tiles.length - PEEK }) }}</button>
          </div>
        </div>
        </template>
      </section>
      <button v-if="query && shownCount < found" class="showmore" data-testid="lib-show-more" @click="showAll = true">{{ t('library.showMore', { n: found - shownCount }) }}</button>

      <section v-if="zoneTypes.length">
        <h2>{{ t('library.zones') }}</h2>
        <div class="grid zones">
          <button v-for="z in zoneTypes" :key="z.id" class="kind" :disabled="editor.readonly" draggable="true" :title="t('library.hint')" @dragstart="dragKind($event, 'zone', z.id)" @click="editor.addZone(z.id)">
            <span class="zchip" :style="{ '--zc': z.color }" aria-hidden="true"><component :is="iconFor(z.icon)" :size="15" :stroke-width="1.9" /></span>
            <span class="truncate">{{ lt(z.name) }}</span>
          </button>
        </div>
      </section>

      <div v-if="!templates.length && !categories.length && !zoneTypes.length" class="empty">{{ t('library.nothing') }}</div>
    </div>

    <Modal v-if="pending" :title="t('library.template.apply.title', { name: lt(pending.meta.name) })" @close="pending = null">
      <p style="margin: 0">{{ t('library.template.apply.text') }}</p>
      <template #footer>
        <button class="btn" @click="pending = null">{{ t('common.cancel') }}</button>
        <button class="btn" @click="applyTemplate('add')">{{ t('library.template.apply.add') }}</button>
        <button class="btn btn-primary" @click="applyTemplate('replace')">{{ t('library.template.apply.replace') }}</button>
      </template>
    </Modal>
  </aside>
</template>

<style scoped>
.library { width: var(--sidebar-w); flex: none; background: var(--surface); border-right: 1px solid var(--line); display: flex; flex-direction: column; min-height: 0; }
.search { position: relative; padding: 14px 14px 10px; display: flex; align-items: center; gap: 6px; }
.collapse { flex: none; width: 30px; height: 30px; border: 0; background: transparent; color: var(--muted); border-radius: var(--radius-sm); display: inline-flex; align-items: center; justify-content: center; }
.collapse:hover { background: var(--surface-sunken); color: var(--text); }
.search > svg:first-child { position: absolute; left: 25px; top: 24px; color: var(--faint); }
.search .input { padding-left: 34px; height: 36px; background: var(--surface-2); flex: 1; min-width: 0; }
.scroll { flex: 1; overflow: auto; padding: 0 14px 18px; }
section { margin-top: 14px; }
h2 { font-size: 11.5px; letter-spacing: .09em; text-transform: uppercase; color: var(--label); font-weight: 600; margin: 0 0 8px 2px; }
h2 .ch { width: 100%; display: flex; align-items: center; gap: 5px; padding: 3px 2px; margin: -3px 0 -3px -2px; border: 0; background: transparent; border-radius: var(--radius-sm); font: inherit; letter-spacing: inherit; text-transform: inherit; color: inherit; text-align: left; }
h2 .ch:hover { color: var(--text); background: var(--surface-sunken); }
h2 .ch .found { margin-left: auto; padding-right: 4px; }
.showmore { width: 100%; margin-top: 12px; height: 34px; border: 1px dashed var(--line-strong); border-radius: var(--radius); background: transparent; color: var(--muted); font-size: 12.5px; }
.showmore:hover { color: var(--accent); border-color: var(--accent-line); }
.tname { min-width: 0; line-height: 1.25; display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2; overflow: hidden; overflow-wrap: anywhere; }
.tpl { width: 100%; display: flex; align-items: center; gap: 8px; min-height: 38px; padding: 6px 12px; margin-bottom: 8px; background: var(--surface); border: 1px solid var(--line-strong); border-radius: var(--radius); font-size: 13.5px; text-align: left; }
.tpl:hover:not(:disabled) { background: var(--surface-2); border-color: #b8c2d4; }
.tpl:disabled { opacity: .55; cursor: not-allowed; }
.badge { font-family: var(--mono); font-size: 11px; color: var(--ok); letter-spacing: .02em; }
.bar { display: flex; gap: 12px; padding: 0 16px 4px; font-size: 11.5px; min-height: 18px; }
.lnk { border: 0; background: transparent; padding: 0; color: var(--muted); font-size: 11.5px; text-decoration: underline; text-underline-offset: 2px; }
.lnk:hover { color: var(--accent); }
.faint { color: var(--faint); }
.found { margin-left: 8px; font-weight: 500; letter-spacing: 0; color: var(--faint); }
.fn { margin-bottom: 4px; }
h2 + .grid { margin-top: 0; }
.fh { width: 100%; display: flex; align-items: center; gap: 5px; height: 28px; padding: 0 4px 0 2px; border: 0; background: transparent; border-radius: var(--radius-sm); font-size: 12.5px; color: var(--text-2); text-align: left; }
.fh:hover { background: var(--surface-sunken); }
.fh .cnt { flex: none; min-width: 18px; text-align: right; font-size: 11px; color: var(--faint); }
.more-tile { grid-column: 1 / -1; justify-content: center; min-height: 30px; color: var(--muted); cursor: pointer; border-style: dashed; }
.more-tile:hover { color: var(--accent); }
/* a product's name is read in full: the tiles take the width they need (one column in a narrow panel, more in a wide one) and a long name wraps to two lines */
.grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(var(--tile-min, 168px), 1fr)); gap: 6px 8px; margin: 2px 0 8px; }
.grid.zones { --tile-min: 104px; }
.nm { display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; overflow-wrap: anywhere; }
.two { display: flex; flex-direction: column; min-width: 0; line-height: 1.2; }
.two .sub { font-size: 10.5px; color: var(--accent); display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
.kind { display: flex; align-items: center; gap: 8px; min-height: 42px; padding: 0 8px 0 7px; background: var(--surface); border: 1px solid var(--line-strong); border-radius: var(--radius); font-size: 12.5px; text-align: left; min-width: 0; cursor: grab; }
.kind:hover:not(:disabled) { background: var(--surface-2); border-color: #b8c2d4; }
.kind:disabled { opacity: .55; cursor: not-allowed; }
.zchip { --zc: var(--line-strong); width: 28px; height: 28px; border-radius: 8px; flex: none; display: inline-flex; align-items: center; justify-content: center; color: color-mix(in srgb, var(--zc) 82%, #000); background: linear-gradient(180deg, color-mix(in srgb, var(--zc) 8%, #fff), color-mix(in srgb, var(--zc) 17%, #fff)); box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--zc) 28%, #fff); }
</style>
