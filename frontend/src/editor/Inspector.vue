<script setup lang="ts">
import { computed, ref } from 'vue'
import { ArrowLeftRight, ChevronDown, ChevronRight, Copy, Trash2, X } from 'lucide-vue-next'
import type { PropDef, Scalar } from '@/api/types'
import { lt, t } from '@/i18n'
import { edgeEffectiveProps, effectiveProps, kindPropDefs } from '@/lib/catalog'
import { findingTitle, statusTagClass, statusLabel } from '@/lib/findings'
import { useApp } from '@/stores/app'
import { useEditor } from '@/stores/editor'
import PropControl from './PropControl.vue'

const app = useApp()
const editor = useEditor()
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

const nodeDefs = computed<PropDef[]>(() => (node.value ? kindPropDefs(ix.value, node.value.kind) : []))
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
const nodeSplit = computed(() => split(nodeDefs.value))
const edgeSplit = computed(() => split(edgeDef.value?.props ?? []))

/** Units (groups of nodes) that a `unit_ref` property may point to. */
function refsFor(def: PropDef): [string, string][] {
  const out = new Map<string, string>()
  for (const n of editor.arch.nodes) {
    const k = ix.value.kinds.get(n.kind)
    if (def.refBehavior && k?.behavior !== def.refBehavior) continue
    const key = n.group || n.id
    if (!out.has(key)) out.set(key, editor.report?.units[key] ?? n.name)
  }
  return [...out]
}

const zoneOptions = computed(() => editor.arch.zones.map((z) => ({ id: z.id, name: z.name })))
const nodeOptions = computed(() => editor.arch.nodes.map((n) => ({ id: n.id, name: n.name })))
const kindOptions = computed(() => (app.boot?.catalog.kinds ?? []).filter((k) => !k.hidden || k.id === node.value?.kind))
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
</script>

<template>
  <div v-if="sel && (node || edge || zone)" class="inspector card" role="region" :aria-label="t('inspector.selected', { name: title })">
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
        <label class="prop"><span class="lbl">{{ t('inspector.kind') }}</span>
          <select class="in" :disabled="dis" :value="node.kind" @change="editor.updateNode(node!.id, { kind: ($event.target as HTMLSelectElement).value, role: null })">
            <option v-for="k in kindOptions" :key="k.id" :value="k.id">{{ lt(k.name) }}</option>
            <option v-if="!kind" :value="node.kind">{{ node.kind }}</option>
          </select>
        </label>
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
            <PropControl v-for="d in g.defs" :key="d.key" :def="d" :value="node.props[d.key]" :fallback="nodeEff[d.key]" :disabled="dis" :refs="d.type === 'unit_ref' ? refsFor(d) : undefined" @change="setNodeProp(d.key, $event)" />
          </template>
        </template>
        <template v-if="nodeSplit.adv.length">
          <button class="more" @click="advanced = !advanced"><component :is="advanced ? ChevronDown : ChevronRight" :size="14" />{{ t('inspector.advanced') }}</button>
          <template v-if="advanced">
            <PropControl v-for="d in nodeSplit.adv" :key="d.key" :def="d" :value="node.props[d.key]" :fallback="nodeEff[d.key]" :disabled="dis" :refs="d.type === 'unit_ref' ? refsFor(d) : undefined" @change="setNodeProp(d.key, $event)" />
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
        <template v-for="g in edgeSplit.groups" :key="g.id">
          <PropControl v-for="d in g.defs" :key="d.key" :def="d" :value="edge.props[d.key]" :fallback="edgeEff[d.key]" :disabled="dis" @change="setEdgeProp(d.key, $event)" />
        </template>
        <template v-if="edgeSplit.adv.length">
          <button class="more" @click="advanced = !advanced"><component :is="advanced ? ChevronDown : ChevronRight" :size="14" />{{ t('inspector.advanced') }}</button>
          <template v-if="advanced">
            <PropControl v-for="d in edgeSplit.adv" :key="d.key" :def="d" :value="edge.props[d.key]" :fallback="edgeEff[d.key]" :disabled="dis" @change="setEdgeProp(d.key, $event)" />
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
        <PropControl v-for="d in zoneDef?.props ?? []" :key="d.key" :def="d" :value="zone.props[d.key]" :disabled="dis" @change="setZoneProp(d.key, $event)" />
        <p class="note">{{ t('inspector.zone.contains', { n: containedNodes }) }}</p>
      </template>

      <template v-if="findings.length">
        <h5>{{ t('inspector.findings') }}</h5>
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
.gh { flex: none; display: flex; align-items: center; gap: 5px; width: 100%; margin-top: 8px; padding: 4px 0; border: 0; background: transparent; text-align: left; font-size: 10.5px; letter-spacing: .09em; text-transform: uppercase; color: #6e7fa8; font-weight: 600; }
.gh:hover { color: var(--accent); }
.gh .cnt { margin-left: auto; font-weight: 500; letter-spacing: 0; color: var(--faint); }
h5 { flex: none; margin: 10px 0 2px; font-size: 10.5px; letter-spacing: .09em; text-transform: uppercase; color: #6e7fa8; font-weight: 600; }
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
.finds { flex: none; list-style: none; margin: 4px 0 0; padding: 0; display: flex; flex-direction: column; gap: 6px; font-size: 12.5px; }
.finds li { display: flex; align-items: flex-start; gap: 8px; }
footer { display: flex; align-items: center; gap: 8px; padding: 8px 12px; border-top: 1px solid var(--line); }
.del { color: var(--crit); }
</style>
