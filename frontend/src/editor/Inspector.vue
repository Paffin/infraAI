<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { ArrowLeftRight, ChevronDown, ChevronRight, Copy, Trash2, X } from 'lucide-vue-next'
import type { PropDef, Scalar } from '@/api/types'
import { lt, t } from '@/i18n'
import { behaviorOf, edgeEffectiveProps, effectiveProps, functionGroups, kindPropDefs, traitsOf, variantOf } from '@/lib/catalog'
import { findingTitle, statusTagClass, statusLabel } from '@/lib/findings'
import { useApp } from '@/stores/app'
import { useEditor } from '@/stores/editor'
import { useUi } from '@/stores/ui'
import { isManaged, versionRefs } from '@/lib/product'
import { useToasts } from '@/stores/toasts'
import { canvasEl, ensureStop, itemFor } from './canvasFocus'
import PropControl from './PropControl.vue'
import VersionField from './VersionField.vue'

const app = useApp()
const editor = useEditor()
const ui = useUi()
const toasts = useToasts()
const advanced = ref(false)
/** Property groups beyond the first two start collapsed so that the card stays compact. */
const groupState = ref<Record<string, boolean>>({})
const isOpen = (id: string, index: number) => groupState.value[id] ?? index < 2
const toggleGroup = (id: string, index: number) => { groupState.value = { ...groupState.value, [id]: !isOpen(id, index) } }

const sel = computed(() => editor.selection)
const ix = computed(() => app.index!)
const node = computed(() => (sel.value?.type === 'node' ? editor.nodeById.get(sel.value.id) : undefined))
const edge = computed(() => (sel.value?.type === 'edge' ? editor.edgeById.get(sel.value.id) : undefined))
const zone = computed(() => (sel.value?.type === 'zone' ? editor.zoneById.get(sel.value.id) : undefined))
const kind = computed(() => (node.value ? ix.value.kinds.get(node.value.kind) : undefined))
const edgeDef = computed(() => (edge.value ? ix.value.edgeTypes.get(edge.value.type) : undefined))
const zoneDef = computed(() => (zone.value ? ix.value.zoneTypes.get(zone.value.type) : undefined))
const dis = computed(() => editor.readonly)

const title = computed(() => node.value?.name ?? (edge.value ? t('inspector.edge') : zone.value?.name ?? ''))

const variant = computed(() => variantOf(kind.value, node.value?.props.variant))
const nodeDefs = computed<PropDef[]>(() => (node.value ? kindPropDefs(ix.value, node.value.kind, node.value.props.variant) : []))
/** The version of the product is part of choosing it: it sits with the product, not in a collapsible group. */
const versionDef = computed(() => nodeDefs.value.find((d) => d.key === 'version'))
/** Release tables that judge the versions of this node (own version and OS / runtime). */
const refs = computed(() => (node.value ? versionRefs(ix.value, app.versions, node.value.kind, node.value.props) : []))
const refOf = (key: string) => refs.value.find((r) => r.prop === key)
const nodeEff = computed(() => (node.value ? effectiveProps(ix.value, node.value.kind, node.value.props) : {}))
const edgeEff = computed(() => (edge.value ? edgeEffectiveProps(ix.value, edge.value.type, edge.value.props) : {}))

function split(defs: PropDef[]) {
  const basic = defs.filter((d) => !d.advanced)
  const adv = defs.filter((d) => d.advanced)
  const groups: { id: string; defs: PropDef[] }[] = []
  for (const d of basic) {
    const g = d.group ?? ''
    let bucket = groups.find((x) => x.id === g)
    if (!bucket) groups.push((bucket = { id: g, defs: [] }))
    bucket.defs.push(d)
  }
  return { groups, adv }
}
const nodeSplit = computed(() => split(nodeDefs.value.filter((d) => d.key !== 'version')))
const edgeSplit = computed(() => split(edgeDef.value?.props ?? []))

/** Units (groups of nodes) that a `unit_ref` property may point to: the right behavior (a consensus cluster) and, when
 * the property asks for it, the right trait (only some consensus products can be a DCS). */
function refsFor(def: PropDef): [string, string][] {
  const out = new Map<string, string>()
  for (const n of editor.arch.nodes) {
    const k = ix.value.kinds.get(n.kind)
    const v = variantOf(k, n.props.variant)
    if (def.refBehavior && behaviorOf(k, v) !== def.refBehavior) continue
    if (def.refTrait && !traitsOf(k, v).has(def.refTrait)) continue
    const key = n.group || n.id
    if (!out.has(key)) out.set(key, editor.report?.units[key] ?? n.name)
  }
  return [...out]
}

const zoneOptions = computed(() => editor.arch.zones.map((z) => ({ id: z.id, name: z.name })))
const nodeOptions = computed(() => editor.arch.nodes.map((n) => ({ id: n.id, name: n.name })))
/** Products grouped by what they do, in the library's order: the user picks "a relational database", then the product. */
const kindGroups = computed(() => {
  const cat = app.boot?.catalog
  if (!cat) return []
  const keep = (k: { id: string; hidden?: boolean }) => !k.hidden || k.id === node.value?.kind
  return [...cat.categories].sort((a, b) => a.order - b.order).flatMap((c) => functionGroups(cat, c.id, keep).map((g) => ({ key: c.id + '/' + g.id, name: g.id ? lt(g.name) : lt(c.name), kinds: g.kinds })))
})
const managed = computed(() => isManaged(variant.value, kind.value))
function changeKind(e: Event) {
  const id = (e.target as HTMLSelectElement).value
  const dropped = editor.changeKind(node.value!.id, id)
  if (dropped.length) toasts.info(t('product.switch.dropped', { keys: dropped.join(', ') }))
}
const edgeTypes = computed(() => app.boot?.catalog.edgeTypes ?? [])
const zoneTypes = computed(() => app.boot?.catalog.zoneTypes ?? [])
const containedNodes = computed(() => (zone.value ? editor.arch.nodes.filter((n) => n.zoneId === zone.value!.id).length : 0))
const findings = computed(() => (sel.value ? editor.findingsOf(sel.value.type === 'node' ? 'nodes' : sel.value.type === 'edge' ? 'edges' : 'zones', sel.value.id) : []))

function setNodeProp(key: string, v: Scalar | undefined) { editor.setProp('node', node.value!.id, key, v) }
function setEdgeProp(key: string, v: Scalar | undefined) { editor.setProp('edge', edge.value!.id, key, v) }
function setZoneProp(key: string, v: Scalar | undefined) { editor.setProp('zone', zone.value!.id, key, v) }

const groupTitle = (g: string) => (g ? t('propgroup.' + g) : '')
function hasGroupTitle(g: string) { return !!g && t('propgroup.' + g) !== 'propgroup.' + g }

function remove() {
  editor.removeSelection()
}

// ------------------------------------------------------------------------- keyboard: the way to the links, and back to the diagram
/** The links of the selected card, each a way to that link: a link is not a stop of the Tab key on the diagram (a diagram has twice as many of them as cards). */
const links = computed(() => {
  const id = node.value?.id
  if (!id) return []
  return editor.shown.edges.filter((e) => e.source === id || e.target === id).map((e) => {
    const out = e.source === id
    const other = out ? e.target : e.source
    const def = ix.value.edgeTypes.get(e.type)
    return { id: e.id, out, name: editor.nodeById.get(other)?.name ?? other, type: def ? lt(def.name) : e.type }
  })
})

/** Open a link from the list of the card: the inspector now shows the link, and the keyboard is on its first way out. */
async function openLink(id: string) {
  editor.select({ type: 'edge', id })
  editor.revealSelection()
  await nextTick()
  root.value?.querySelector<HTMLElement>('[data-go]')?.focus()
}

/** From a link to one of its ends: that card is selected, in view and has the keyboard. */
async function goTo(nodeId: string) {
  editor.select({ type: 'node', id: nodeId })
  editor.revealSelection()
  await nextTick()
  const canvas = canvasEl()
  itemFor(canvas ?? document, { type: 'node', id: nodeId })?.focus({ preventScroll: true })
}

/** Escape in the inspector: back to the card (or the end of the link) it is about; the selection stays. */
function backToDiagram(e: KeyboardEvent) {
  if (e.defaultPrevented) return
  const canvas = canvasEl()
  if (!canvas) return
  const s = sel.value
  const anchor = s?.type === 'edge' ? { type: 'node' as const, id: edge.value?.source ?? '' } : s
  const target = itemFor(canvas, anchor) ?? ensureStop(canvas)
  if (!target) return
  e.stopPropagation()
  target.focus({ preventScroll: true })
}

// ------------------------------------------------------------------------- "fill this in" requests
const root = ref<HTMLElement | null>(null)
const flashKey = ref('')
let flashTimer: ReturnType<typeof setTimeout> | undefined

/** The user was sent to a field (a "no data" finding names it): open its group, scroll to it, focus it, flash it. */
async function applyFocus() {
  const r = ui.focusRequest
  const s = sel.value
  if (!r || r.need.target === 'requirements' || !s || s.type !== r.need.target || s.id !== r.need.id) return
  ui.focusRequest = null
  const key = r.need.prop
  if (node.value) {
    const gi = nodeSplit.value.groups.findIndex((g) => g.defs.some((d) => d.key === key))
    if (gi >= 0) { const g = nodeSplit.value.groups[gi]; if (!isOpen(g.id, gi)) toggleGroup(g.id, gi) }
    else if (nodeSplit.value.adv.some((d) => d.key === key)) advanced.value = true
  } else if (edge.value && edgeSplit.value.adv.some((d) => d.key === key)) advanced.value = true
  await nextTick()
  const row = root.value?.querySelector<HTMLElement>(`[data-prop="${CSS.escape(key)}"]`)
  if (!row) return
  row.scrollIntoView({ block: 'center', behavior: 'smooth' })
  row.querySelector<HTMLElement>('input, select')?.focus({ preventScroll: true })
  flashKey.value = key
  clearTimeout(flashTimer)
  flashTimer = setTimeout(() => { flashKey.value = '' }, 2300)
}
watch(() => ui.focusRequest, () => void nextTick(applyFocus), { immediate: true })
</script>

<template>
  <div v-if="sel && (node || edge || zone)" ref="root" class="inspector card" role="region" :aria-label="t('inspector.selected', { name: title })" @keydown.esc="backToDiagram">
    <header>
      <span class="dot" :class="edge ? 'edge' : zone ? 'zone' : ''" />
      <b class="grow truncate">{{ t('inspector.selected', { name: title }) }}</b>
      <button class="btn btn-ghost btn-icon" :aria-label="t('common.close')" @click="editor.select(null)"><X :size="16" /></button>
    </header>

    <div class="body">
      <p v-if="node && editor.previewSets?.added.nodes.has(node.id)" class="note">{{ t('inspector.proposed') }}</p>

      <!-- ===================================================================== node -->
      <template v-if="node">
        <label class="prop"><span class="lbl">{{ t('inspector.name') }}</span>
          <input class="in" :disabled="dis" :value="node.name" @input="editor.updateNode(node!.id, { name: ($event.target as HTMLInputElement).value })" />
        </label>
        <label class="prop"><span class="lbl">{{ t('product.title') }}</span>
          <select class="in" :disabled="dis" :value="node.kind" @change="changeKind">
            <optgroup v-for="g in kindGroups" :key="g.key" :label="g.name">
              <option v-for="k in g.kinds" :key="k.id" :value="k.id">{{ lt(k.name) }}</option>
            </optgroup>
            <option v-if="!kind" :value="node.kind">{{ node.kind }}</option>
          </select>
        </label>
        <template v-if="kind?.variants?.length">
          <label class="prop" :class="{ assumed: !node.props.variant }"><span class="lbl">{{ t('product.variant') }}</span>
            <select class="in" :disabled="dis" :value="node.props.variant ? String(node.props.variant) : ''" data-prop="variant" @change="setNodeProp('variant', (($event.target as HTMLSelectElement).value || undefined))">
              <option value="">{{ t('product.variant.default') }}</option>
              <option v-for="v in kind.variants" :key="v.id" :value="v.id">{{ lt(v.name) }}</option>
            </select>
          </label>
          <p v-if="variant?.description" class="vdesc"><b v-if="!node.props.variant">{{ lt(variant.name) }}. </b>{{ lt(variant.description) }}<span v-if="managed" class="tag tag-nodata mgd" :title="t('product.variant.managedHint')">{{ t('product.variant.managed') }}</span></p>
        </template>
        <VersionField v-if="versionDef" :def="versionDef" :value="node.props.version" :fallback="nodeEff.version" :disabled="dis" :flash="flashKey === 'version'" :table="refOf('version')" :versions="app.versions" @change="setNodeProp('version', $event)" />
        <label class="prop"><span class="lbl">{{ t('inspector.zoneOf') }}</span>
          <select class="in" :disabled="dis" :value="node.zoneId ?? ''" @change="editor.updateNode(node!.id, { zoneId: ($event.target as HTMLSelectElement).value || null })">
            <option value="">{{ t('inspector.global') }}</option>
            <option v-for="z in zoneOptions" :key="z.id" :value="z.id">{{ z.name }}</option>
          </select>
        </label>
        <label class="prop" :title="t('inspector.group.hint')"><span class="lbl">{{ t('inspector.group') }}</span>
          <input class="in mono" list="ia-groups" :disabled="dis" :value="node.group ?? ''" :placeholder="t('inspector.unset')" @input="editor.updateNode(node!.id, { group: ($event.target as HTMLInputElement).value })" />
        </label>
        <datalist id="ia-groups"><option v-for="g in editor.groups" :key="g" :value="g" /></datalist>
        <label v-if="kind && kind.roles.length" class="prop"><span class="lbl">{{ t('inspector.role') }}</span>
          <select class="in" :disabled="dis" :value="node.role ?? ''" @change="editor.updateNode(node!.id, { role: ($event.target as HTMLSelectElement).value || null })">
            <option value="">{{ t('inspector.role.none') }}</option>
            <option v-for="r in kind.roles" :key="r.id" :value="r.id">{{ lt(r.name) }}</option>
          </select>
        </label>

        <template v-for="(g, gi) in nodeSplit.groups" :key="g.id">
          <button v-if="hasGroupTitle(g.id)" class="gh" :aria-expanded="isOpen(g.id, gi)" @click="toggleGroup(g.id, gi)"><component :is="isOpen(g.id, gi) ? ChevronDown : ChevronRight" :size="13" />{{ groupTitle(g.id) }}<span class="cnt">{{ g.defs.length }}</span></button>
          <template v-if="!hasGroupTitle(g.id) || isOpen(g.id, gi)">
            <template v-for="d in g.defs" :key="d.key">
              <VersionField v-if="d.lifecycle" :def="d" :value="node.props[d.key]" :fallback="nodeEff[d.key]" :disabled="dis" :flash="flashKey === d.key" :table="refOf(d.key)" :versions="app.versions" @change="setNodeProp(d.key, $event)" />
              <PropControl v-else :def="d" :value="node.props[d.key]" :fallback="nodeEff[d.key]" :disabled="dis" :flash="flashKey === d.key" :refs="d.type === 'unit_ref' ? refsFor(d) : undefined" @change="setNodeProp(d.key, $event)" />
            </template>
          </template>
        </template>
        <template v-if="nodeSplit.adv.length">
          <button class="more" @click="advanced = !advanced"><component :is="advanced ? ChevronDown : ChevronRight" :size="14" />{{ t('inspector.advanced') }}</button>
          <template v-if="advanced">
            <template v-for="d in nodeSplit.adv" :key="d.key">
              <VersionField v-if="d.lifecycle" :def="d" :value="node.props[d.key]" :fallback="nodeEff[d.key]" :disabled="dis" :flash="flashKey === d.key" :table="refOf(d.key)" :versions="app.versions" @change="setNodeProp(d.key, $event)" />
              <PropControl v-else :def="d" :value="node.props[d.key]" :fallback="nodeEff[d.key]" :disabled="dis" :flash="flashKey === d.key" :refs="d.type === 'unit_ref' ? refsFor(d) : undefined" @change="setNodeProp(d.key, $event)" />
            </template>
          </template>
        </template>
      </template>

      <!-- ===================================================================== edge -->
      <template v-else-if="edge">
        <label class="prop"><span class="lbl">{{ t('inspector.type') }}</span>
          <select class="in" :disabled="dis" :value="edge.type" @change="editor.updateEdge(edge!.id, { type: ($event.target as HTMLSelectElement).value })">
            <option v-for="e in edgeTypes" :key="e.id" :value="e.id">{{ lt(e.name) }}</option>
            <option v-if="!edgeDef" :value="edge.type">{{ edge.type }}</option>
          </select>
        </label>
        <label class="prop"><span class="lbl">{{ t('inspector.label') }}</span>
          <input class="in" :disabled="dis" :value="edge.label ?? ''" :placeholder="t('inspector.unset')" @input="editor.updateEdge(edge!.id, { label: ($event.target as HTMLInputElement).value })" />
        </label>
        <label class="prop"><span class="lbl">{{ t('inspector.source') }}</span>
          <select class="in" :disabled="dis" :value="edge.source" @change="editor.updateEdge(edge!.id, { source: ($event.target as HTMLSelectElement).value })">
            <option v-for="n in nodeOptions" :key="n.id" :value="n.id">{{ n.name }}</option>
          </select>
        </label>
        <label class="prop"><span class="lbl">{{ t('inspector.target') }}</span>
          <select class="in" :disabled="dis" :value="edge.target" @change="editor.updateEdge(edge!.id, { target: ($event.target as HTMLSelectElement).value })">
            <option v-for="n in nodeOptions" :key="n.id" :value="n.id">{{ n.name }}</option>
          </select>
        </label>
        <button class="more" :disabled="dis" @click="editor.reverseEdge(edge!.id)"><ArrowLeftRight :size="14" />{{ t('inspector.reverse') }}</button>
        <button class="more" data-go="source" @click="goTo(edge!.source)">{{ t('inspector.go.source', { name: editor.nodeById.get(edge!.source)?.name ?? edge!.source }) }}</button>
        <button class="more" data-go="target" @click="goTo(edge!.target)">{{ t('inspector.go.target', { name: editor.nodeById.get(edge!.target)?.name ?? edge!.target }) }}</button>
        <template v-for="g in edgeSplit.groups" :key="g.id">
          <PropControl v-for="d in g.defs" :key="d.key" :def="d" :value="edge.props[d.key]" :fallback="edgeEff[d.key]" :disabled="dis" :flash="flashKey === d.key" @change="setEdgeProp(d.key, $event)" />
        </template>
        <template v-if="edgeSplit.adv.length">
          <button class="more" @click="advanced = !advanced"><component :is="advanced ? ChevronDown : ChevronRight" :size="14" />{{ t('inspector.advanced') }}</button>
          <template v-if="advanced">
            <PropControl v-for="d in edgeSplit.adv" :key="d.key" :def="d" :value="edge.props[d.key]" :fallback="edgeEff[d.key]" :disabled="dis" :flash="flashKey === d.key" @change="setEdgeProp(d.key, $event)" />
          </template>
        </template>
      </template>

      <!-- ===================================================================== zone -->
      <template v-else-if="zone">
        <label class="prop"><span class="lbl">{{ t('inspector.name') }}</span>
          <input class="in" :disabled="dis" :value="zone.name" @input="editor.updateZone(zone!.id, { name: ($event.target as HTMLInputElement).value })" />
        </label>
        <label class="prop"><span class="lbl">{{ t('inspector.zone.type') }}</span>
          <select class="in" :disabled="dis" :value="zone.type" @change="editor.updateZone(zone!.id, { type: ($event.target as HTMLSelectElement).value })">
            <option v-for="z in zoneTypes" :key="z.id" :value="z.id">{{ lt(z.name) }}</option>
          </select>
        </label>
        <label class="prop"><span class="lbl">{{ t('inspector.zone.role') }}</span>
          <select class="in" :disabled="dis" :value="zone.role ?? ''" @change="editor.updateZone(zone!.id, { role: ($event.target as HTMLSelectElement).value || null })">
            <option value="">{{ t('inspector.role.none') }}</option>
            <option value="active">{{ t('zone.role.active') }}</option>
            <option value="standby">{{ t('zone.role.standby') }}</option>
            <option value="witness">{{ t('zone.role.witness') }}</option>
          </select>
        </label>
        <label class="prop"><span class="lbl">{{ t('inspector.zone.parent') }}</span>
          <select class="in" :disabled="dis" :value="zone.parentId ?? ''" @change="editor.updateZone(zone!.id, { parentId: ($event.target as HTMLSelectElement).value || null })">
            <option value="">{{ t('inspector.zone.none') }}</option>
            <option v-for="z in zoneOptions.filter((x) => x.id !== zone!.id)" :key="z.id" :value="z.id">{{ z.name }}</option>
          </select>
        </label>
        <PropControl v-for="d in zoneDef?.props ?? []" :key="d.key" :def="d" :value="zone.props[d.key]" :disabled="dis" :flash="flashKey === d.key" @change="setZoneProp(d.key, $event)" />
        <p class="note">{{ t('inspector.zone.contains', { n: containedNodes }) }}</p>
      </template>

      <template v-if="links.length">
        <h3>{{ t('inspector.links') }}</h3>
        <ul class="links">
          <li v-for="l in links" :key="l.id">
            <button class="link" data-testid="inspector-link" @click="openLink(l.id)"><span class="arrow" aria-hidden="true">{{ l.out ? '→' : '←' }}</span><span class="grow truncate">{{ t(l.out ? 'inspector.link.out' : 'inspector.link.in', { name: l.name }) }}</span><span class="faint">{{ l.type }}</span></button>
          </li>
        </ul>
      </template>

      <template v-if="findings.length">
        <h3>{{ t('inspector.findings') }}</h3>
        <ul class="finds">
          <li v-for="f in findings" :key="f.id"><span class="tag" :class="statusTagClass(f.status)">{{ statusLabel(f.status) }}</span><span class="grow">{{ findingTitle(app.rules.get(f.ruleId), f) }}</span></li>
        </ul>
      </template>
    </div>

    <footer v-if="!dis">
      <button v-if="node" class="btn btn-sm" @click="editor.duplicateNode(node!.id)"><Copy :size="14" />{{ t('inspector.duplicate') }}</button>
      <span class="spacer" />
      <button class="btn btn-sm btn-ghost del" @click="remove"><Trash2 :size="14" />{{ t('inspector.delete') }}</button>
    </footer>
  </div>
</template>

<style scoped>
.inspector { position: absolute; left: 18px; top: 62px; width: 308px; max-height: calc(100% - 150px); display: flex; flex-direction: column; z-index: 55; box-shadow: var(--shadow-2); }
header { display: flex; align-items: center; gap: 8px; padding: 10px 8px 6px 16px; font-size: 14px; }
.dot { width: 9px; height: 9px; border-radius: 50%; background: var(--accent); flex: none; }
.dot.edge { background: var(--edge-sync); }
.dot.zone { background: var(--edge-quorum); }
.body { overflow: auto; padding: 0 14px 10px 16px; display: flex; flex-direction: column; gap: 2px; }
.gh { flex: none; display: flex; align-items: center; gap: 5px; width: 100%; margin-top: 8px; padding: 4px 0; border: 0; background: transparent; text-align: left; font-size: 10.5px; letter-spacing: .09em; text-transform: uppercase; color: var(--label); font-weight: 600; }
.gh:hover { color: var(--accent); }
.gh .cnt { margin-left: auto; font-weight: 500; letter-spacing: 0; color: var(--faint); }
h3 { flex: none; margin: 10px 0 2px; font-size: 10.5px; letter-spacing: .09em; text-transform: uppercase; color: var(--label); font-weight: 600; }
.prop { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1.05fr); gap: 10px; align-items: center; min-height: 30px; font-size: 12.5px; flex: none; }
.lbl { color: var(--muted); }
.in { width: 100%; height: 28px; padding: 0 8px; border: 1px solid transparent; border-radius: var(--radius-sm); background: transparent; text-align: right; font-size: 12.5px; color: var(--text); }
.in.mono { font-family: var(--mono); font-size: 12px; }
select.in { text-align-last: right; padding-right: 2px; }
.in:hover:not(:disabled) { border-color: var(--line-strong); background: var(--surface); }
.in:focus { outline: none; border-color: var(--accent); background: var(--surface); box-shadow: 0 0 0 3px var(--accent-soft); }
.in:disabled { cursor: default; -webkit-text-fill-color: currentColor; opacity: 1; }
.in::placeholder { color: var(--faint); }
.more { flex: none; display: inline-flex; align-items: center; gap: 5px; align-self: flex-start; margin-top: 8px; padding: 2px 4px; border: 0; background: transparent; color: var(--muted); font-size: 12px; border-radius: 4px; }
.more:hover:not(:disabled) { color: var(--text); background: var(--surface-sunken); }
.note { flex: none; margin: 6px 0 2px; font-size: 12px; color: var(--muted); }
.assumed .in:not(:focus) { color: var(--faint); }
.vdesc { flex: none; margin: 0 0 4px; font-size: 11.5px; line-height: 1.4; color: var(--muted); }
.mgd { margin-left: 6px; font-size: 10.5px; vertical-align: 1px; }
.links { flex: none; list-style: none; margin: 2px 0 0; padding: 0; display: flex; flex-direction: column; gap: 1px; }
.link { width: 100%; display: flex; align-items: center; gap: 8px; min-height: 28px; padding: 2px 6px; border: 0; border-radius: var(--radius-sm); background: transparent; text-align: left; font-size: 12.5px; color: var(--text); }
.link:hover { background: var(--surface-sunken); }
.link .arrow { flex: none; color: var(--muted); }
.link .faint { flex: none; font-size: 11.5px; max-width: 45%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.finds { flex: none; list-style: none; margin: 4px 0 0; padding: 0; display: flex; flex-direction: column; gap: 6px; font-size: 12.5px; }
.finds li { display: flex; align-items: flex-start; gap: 8px; }
footer { display: flex; align-items: center; gap: 8px; padding: 8px 12px; border-top: 1px solid var(--line); }
.del { color: var(--crit); }
@container (max-width: 1260px) { .inspector { top: 104px; max-height: calc(100% - 280px); } }
</style>
