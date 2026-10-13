<script setup lang="ts">
import { computed, ref } from 'vue'
import { ChevronDown, ChevronRight, PanelLeftClose, Search } from 'lucide-vue-next'
import * as api from '@/api/endpoints'
import type { KindDef, TemplateMeta, Architecture, VariantDef } from '@/api/types'
import Modal from '@/components/Modal.vue'
import { lt, t } from '@/i18n'
import { functionGroups } from '@/lib/catalog'
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

const categories = computed(() => {
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
const found = computed(() => categories.value.reduce((n, c) => n + c.count, 0))

// Groups with many products start folded: a hundred buttons in one list help nobody. A search opens everything it finds.
const OPEN_KEY = 'ia_lib_open'
function loadOpen(): Record<string, boolean> {
  try { return JSON.parse(localStorage.getItem(OPEN_KEY) ?? '{}') } catch { return {} }
}
const openState = ref<Record<string, boolean>>(loadOpen())
const SMALL_GROUP = 2
const isOpen = (key: string, size: number) => !!query.value || (openState.value[key] ?? size <= SMALL_GROUP)
function toggle(key: string, size: number) {
  openState.value = { ...openState.value, [key]: !isOpen(key, size) }
  try { localStorage.setItem(OPEN_KEY, JSON.stringify(openState.value)) } catch { /* private mode */ }
}
function setAll(open: boolean) {
  const next: Record<string, boolean> = {}
  for (const c of categories.value) for (const g of c.fns) next[g.key] = open
  openState.value = next
  try { localStorage.setItem(OPEN_KEY, JSON.stringify(next)) } catch { /* private mode */ }
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
  <aside v-show="ui.libraryOpen" class="library">
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
        <h4>{{ t('library.templates') }}</h4>
        <button v-for="tpl in templates" :key="tpl.id" class="tpl" :disabled="editor.readonly" :title="lt(tpl.description)" @click="pickTemplate(tpl)">
          <span class="grow truncate">{{ lt(tpl.name) }}</span>
          <span v-if="tpl.badge" class="badge">{{ tpl.badge }} ✓</span>
        </button>
      </section>

      <section v-for="c in categories" :key="c.id">
        <h4>{{ c.name }}<span v-if="query" class="found">{{ c.count }}</span></h4>
        <div v-for="g in c.fns" :key="g.key" class="fn">
          <button v-if="g.name" class="fh" :aria-expanded="isOpen(g.key, g.tiles.length)" @click="toggle(g.key, g.tiles.length)">
            <component :is="isOpen(g.key, g.tiles.length) ? ChevronDown : ChevronRight" :size="13" />
            <span class="grow truncate">{{ g.name }}</span>
            <span v-if="!isOpen(g.key, g.tiles.length)" class="peek" aria-hidden="true"><i v-for="x in g.tiles.slice(0, 5)" :key="x.kind.id" :style="{ background: x.kind.color }" /></span>
            <span class="cnt">{{ g.tiles.length }}</span>
          </button>
          <div v-if="isOpen(g.key, g.tiles.length)" class="grid">
            <button
              v-for="x in g.tiles" :key="x.kind.id" class="kind" :disabled="editor.readonly" draggable="true" :title="tileTitle(x.kind)"
              @dragstart="dragKind($event, 'kind', x.kind.id, x.variant?.id)" @click="addKind(x)"
            >
              <span class="badge-abbr" :style="{ background: x.kind.color }">{{ x.kind.abbr }}</span>
              <span class="two">
                <span class="truncate">{{ lt(x.kind.name) }}</span>
                <span v-if="x.variant" class="sub truncate">{{ t('library.matchVariant', { name: lt(x.variant.name) }) }}</span>
              </span>
            </button>
          </div>
        </div>
      </section>

      <section v-if="zoneTypes.length">
        <h4>{{ t('library.zones') }}</h4>
        <div class="grid">
          <button v-for="z in zoneTypes" :key="z.id" class="kind" :disabled="editor.readonly" draggable="true" :title="t('library.hint')" @dragstart="dragKind($event, 'zone', z.id)" @click="editor.addZone(z.id)">
            <span class="zsw" :style="{ borderColor: z.color }" />
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
h4 { font-size: 11.5px; letter-spacing: .09em; text-transform: uppercase; color: #6e7fa8; font-weight: 600; margin: 0 0 8px 2px; }
.tpl { width: 100%; display: flex; align-items: center; gap: 8px; height: 38px; padding: 0 12px; margin-bottom: 8px; background: var(--surface); border: 1px solid var(--line-strong); border-radius: var(--radius); font-size: 13.5px; text-align: left; }
.tpl:hover:not(:disabled) { background: var(--surface-2); border-color: #b8c2d4; }
.tpl:disabled { opacity: .55; cursor: not-allowed; }
.badge { font-family: var(--mono); font-size: 11px; color: var(--ok); letter-spacing: .02em; }
.bar { display: flex; gap: 12px; padding: 0 16px 4px; font-size: 11.5px; min-height: 18px; }
.lnk { border: 0; background: transparent; padding: 0; color: var(--muted); font-size: 11.5px; text-decoration: underline; text-underline-offset: 2px; }
.lnk:hover { color: var(--accent); }
.faint { color: var(--faint); }
.found { margin-left: 8px; font-weight: 500; letter-spacing: 0; color: var(--faint); }
.fn { margin-bottom: 4px; }
.fh { width: 100%; display: flex; align-items: center; gap: 5px; height: 28px; padding: 0 4px 0 2px; border: 0; background: transparent; border-radius: var(--radius-sm); font-size: 12.5px; color: var(--text-2); text-align: left; }
.fh:hover { background: var(--surface-sunken); }
.fh .cnt { flex: none; min-width: 18px; text-align: right; font-size: 11px; color: var(--faint); }
.peek { flex: none; display: inline-flex; gap: 3px; }
.peek i { width: 9px; height: 9px; border-radius: 3px; opacity: .85; }
.grid { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; margin: 2px 0 8px; }
.two { display: flex; flex-direction: column; min-width: 0; line-height: 1.2; }
.two .sub { font-size: 10.5px; color: var(--accent); }
.kind { display: flex; align-items: center; gap: 7px; min-height: 40px; padding: 0 7px; background: var(--surface); border: 1px solid var(--line-strong); border-radius: var(--radius); font-size: 12.5px; text-align: left; min-width: 0; cursor: grab; }
.kind:hover:not(:disabled) { background: var(--surface-2); border-color: #b8c2d4; }
.kind:disabled { opacity: .55; cursor: not-allowed; }
.kind .badge-abbr { flex: none; min-width: 26px; height: 24px; font-size: 9px; }
.zsw { width: 26px; height: 18px; border: 2px solid; border-radius: 5px; flex: none; }
</style>
