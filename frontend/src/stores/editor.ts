import { defineStore } from 'pinia'
import { computed, ref, shallowRef, watch } from 'vue'
import { ApiError } from '@/api/client'
import * as api from '@/api/endpoints'
import type {
  ArchDiff, ArchNode, Architecture, Edge, Finding, Op, ProjectDetail, Props, Report, Requirements, Scalar, Scenario, SimResult, Status, Waiver, Zone,
} from '@/api/types'
import { CARD_H, CARD_W, DEFAULT_ZONE_H, DEFAULT_ZONE_W, boundsOf, layoutAll, mergeArchitecture, nodeRect, placeInZone, placeMissing, zoneAt, zoneChain, zoneDescendants, zoneRect } from '@/editor/layout'
import { lang, lt, t } from '@/i18n'
import { effectiveProps, kindPropDefs } from '@/lib/catalog'
import { errorMessage, newId } from '@/lib/format'
import { statusRank } from '@/lib/findings'
import { useApp } from './app'
import { useToasts } from './toasts'

export type SaveState = 'saved' | 'dirty' | 'saving' | 'conflict' | 'error' | 'readonly'
export type Tool = 'select' | 'connect' | 'zone'
export type Layer = 'all' | 'network' | 'data' | 'dr'
export type Selection = { type: 'node' | 'edge' | 'zone'; id: string } | null

export interface Preview {
  /** `proposal`: a pending AI proposal; `compare`: a read-only comparison (version, imported snapshot) */
  kind?: 'proposal' | 'compare'
  label?: string
  proposalId: string
  base: Architecture
  arch: Architecture
  diff: ArchDiff
}

export const emptyArchitecture = (): Architecture => ({ schemaVersion: 1, meta: {}, requirements: {}, zones: [], nodes: [], edges: [] })
const clone = <T>(x: T): T => JSON.parse(JSON.stringify(x))

export const useEditor = defineStore('editor', () => {
  const app = useApp()
  const toasts = useToasts()

  /** Layout options from the editor settings (sites per row). */
  const layoutOpts = () => ({ zonesPerRow: app.editorCfg.layoutZonesPerRow })

  // ------------------------------------------------------------------------------------- state
  const projectId = ref('')
  const project = ref<ProjectDetail['project'] | null>(null)
  const arch = shallowRef<Architecture>(emptyArchitecture())
  const rev = ref(0)
  const waivers = ref<Waiver[]>([])
  const loading = ref(false)
  const loadError = ref('')
  const saveState = ref<SaveState>('saved')
  const saveError = ref('')
  const serverRev = ref<number | null>(null)

  const report = shallowRef<Report | null>(null)
  const evaluating = ref(false)
  const evalError = ref('')

  const selection = ref<Selection>(null)
  /** Nodes selected together on the canvas (Shift+click, Shift+drag): two or more get the bulk-actions card. */
  const multi = ref<string[]>([])
  /** Ask the canvas to select these nodes (it owns the selection state of the flow). */
  const selectRequest = ref<{ ids: string[]; token: number } | null>(null)
  const tool = ref<Tool>('select')
  const layer = ref<Layer>('all')
  const highlight = ref<{ nodes: string[]; edges: string[]; zones: string[]; token: number } | null>(null)
  const preview = shallowRef<Preview | null>(null)
  const viewCenter = ref<{ x: number; y: number }>({ x: 400, y: 300 })
  const fitToken = ref(0)
  /** Ask the canvas to pan/zoom to elements (without dimming the rest, unlike `highlight`). */
  const reveal = ref<{ nodes: string[]; zones: string[]; token: number } | null>(null)

  const sim = ref<{ scenario: Scenario; result: SimResult | null; loading: boolean; error: string } | null>(null)
  const pickingNode = ref(false)
  const connectFrom = ref<string | null>(null)

  const past: Architecture[] = []
  const future: Architecture[] = []
  const historyVersion = ref(0)
  let lastKey = ''
  let lastAt = 0

  watch(selection, (s) => { if (s && multi.value.length) multi.value = [] })

  // ------------------------------------------------------------------------------------ derived
  const readonly = computed(() => !app.canEdit || preview.value !== null)
  const canUndo = computed(() => historyVersion.value >= 0 && past.length > 0 && !readonly.value)
  const canRedo = computed(() => historyVersion.value >= 0 && future.length > 0 && !readonly.value)
  const isEmpty = computed(() => arch.value.nodes.length === 0 && arch.value.zones.length === 0)

  /** The architecture the canvas displays: the working copy, or the pending AI proposal laid over it. */
  const shown = computed<Architecture>(() => {
    const p = preview.value
    if (!p) return arch.value
    const out = clone(p.arch)
    const have = new Set([...out.nodes.map((n) => n.id), ...out.zones.map((z) => z.id), ...out.edges.map((e) => e.id)])
    for (const z of p.base.zones) if (!have.has(z.id) && p.diff.zones.removed.some((r) => r.id === z.id)) out.zones.push(z)
    for (const n of p.base.nodes) if (!have.has(n.id) && p.diff.nodes.removed.some((r) => r.id === n.id)) out.nodes.push(n)
    for (const e of p.base.edges) if (!have.has(e.id) && p.diff.edges.removed.some((r) => r.id === e.id)) out.edges.push(e)
    return placeMissing(out, app.index!, layoutOpts())
  })
  const previewSets = computed(() => {
    const p = preview.value
    const ids = (l: { id: string }[]) => new Set(l.map((x) => x.id))
    return p
      ? { added: { nodes: ids(p.diff.nodes.added), edges: ids(p.diff.edges.added), zones: ids(p.diff.zones.added) }, removed: { nodes: ids(p.diff.nodes.removed), edges: ids(p.diff.edges.removed), zones: ids(p.diff.zones.removed) }, changed: { nodes: ids(p.diff.nodes.changed), edges: ids(p.diff.edges.changed), zones: ids(p.diff.zones.changed) } }
      : null
  })

  const nodeById = computed(() => new Map(shown.value.nodes.map((n) => [n.id, n])))
  const edgeById = computed(() => new Map(shown.value.edges.map((e) => [e.id, e])))
  const zoneById = computed(() => new Map(shown.value.zones.map((z) => [z.id, z])))

  const openFindings = computed(() => (report.value?.findings ?? []).filter((f) => !f.accepted && f.status !== 'ok' && f.status !== 'na'))

  function worstBy(kind: 'nodes' | 'edges' | 'zones'): Map<string, Status> {
    const m = new Map<string, Status>()
    for (const f of openFindings.value) {
      for (const id of f.subjects[kind]) {
        const cur = m.get(id)
        if (!cur || statusRank[f.status] < statusRank[cur]) m.set(id, f.status)
      }
    }
    return m
  }
  const nodeStatus = computed(() => worstBy('nodes'))
  const edgeStatus = computed(() => worstBy('edges'))
  const zoneStatus = computed(() => worstBy('zones'))
  const findingsOf = (type: 'nodes' | 'edges' | 'zones', id: string): Finding[] => openFindings.value.filter((f) => f.subjects[type].includes(id))

  const groups = computed(() => [...new Set(arch.value.nodes.map((n) => n.group).filter((g): g is string => !!g))].sort())
  const siteZones = computed(() => arch.value.zones.filter((z) => app.index?.zoneTypes.get(z.type)?.level === 'site'))
  const rackZones = computed(() => arch.value.zones.filter((z) => app.index?.zoneTypes.get(z.type)?.level === 'rack'))

  // ------------------------------------------------------------------------------------ loading
  async function load(id: string) {
    reset()
    projectId.value = id
    loading.value = true
    try {
      const d = await api.projects.get(id)
      project.value = d.project
      rev.value = d.rev
      waivers.value = d.waivers
      const placed = placeMissing(d.architecture, app.index!, layoutOpts())
      arch.value = placed
      saveState.value = app.canEdit ? 'saved' : 'readonly'
      if (placed !== d.architecture && app.canEdit) touchSave()
      fitToken.value++
      void evaluateNow()
    } catch (e) {
      loadError.value = errorMessage(e)
    } finally {
      loading.value = false
    }
  }

  function reset() {
    clearTimeout(saveTimer)
    clearTimeout(evalTimer)
    clearTimeout(simTimer)
    evalAbort?.abort()
    projectId.value = ''
    project.value = null
    arch.value = emptyArchitecture()
    rev.value = 0
    waivers.value = []
    report.value = null
    selection.value = null
    multi.value = []
    selectRequest.value = null
    tool.value = 'select'
    preview.value = null
    sim.value = null
    pickingNode.value = false
    connectFrom.value = null
    highlight.value = null
    reveal.value = null
    loadError.value = ''
    saveState.value = 'saved'
    saveError.value = ''
    serverRev.value = null
    past.length = 0
    future.length = 0
    historyVersion.value++
  }

  // -------------------------------------------------------------------------------------- saving
  let saveTimer: ReturnType<typeof setTimeout> | undefined
  let inflight: Promise<boolean> | null = null
  let retryDelay = 0 // the pause before the next retry; 0 = start from `editor.saveRetryMs`

  function touchSave() {
    if (!app.canEdit || saveState.value === 'conflict') return
    saveState.value = 'dirty'
    clearTimeout(saveTimer)
    saveTimer = setTimeout(() => void flushSave(), app.editorCfg.autosaveMs)
  }

  /** Store the working copy now; resolves when everything that was edited so far is on the server. */
  async function flushSave(): Promise<boolean> {
    clearTimeout(saveTimer)
    if (!projectId.value || !app.canEdit) return true
    for (let round = 0; round < 3; round++) {
      if (saveState.value === 'conflict') return false
      if (inflight) {
        if (!(await inflight)) return false
        continue
      }
      if (saveState.value === 'saved') return true
      inflight = persist().finally(() => { inflight = null })
      if (!(await inflight)) return false
    }
    return saveState.value === 'saved'
  }

  async function persist(): Promise<boolean> {
    saveState.value = 'saving'
    const snapshot = arch.value
    try {
      const r = await api.projects.saveDraft(projectId.value, snapshot, rev.value)
      rev.value = r.rev
      retryDelay = 0
      saveError.value = ''
      saveState.value = arch.value === snapshot ? 'saved' : 'dirty'
      return true
    } catch (e) {
      if (e instanceof ApiError && e.status === 409) {
        serverRev.value = (e.details?.currentRev as number) ?? null
        saveState.value = 'conflict'
      } else {
        saveState.value = 'error'
        saveError.value = errorMessage(e)
        // transient failures (network, 5xx) are retried with a growing delay; validation errors are not
        if (!(e instanceof ApiError) || e.status === 0 || e.status >= 500) {
          const wait = retryDelay || app.editorCfg.saveRetryMs
          saveTimer = setTimeout(() => { saveState.value = 'dirty'; void flushSave() }, wait)
          retryDelay = Math.min(wait * 2, app.editorCfg.saveRetryMaxMs)
        }
      }
      return false
    }
  }

  /** Discard local changes and take the server's version (after a conflict). */
  async function reloadFromServer() {
    const id = projectId.value
    clearTimeout(saveTimer)
    saveState.value = 'saved'
    await load(id)
  }

  // ------------------------------------------------------------------------------------ evaluate
  let evalTimer: ReturnType<typeof setTimeout> | undefined
  let evalAbort: AbortController | undefined
  let evalSeq = 0

  function scheduleEvaluate() {
    clearTimeout(evalTimer)
    evalTimer = setTimeout(() => void evaluateNow(), app.editorCfg.evaluateMs)
  }

  async function evaluateNow() {
    if (!projectId.value) return
    evalAbort?.abort()
    const ctl = new AbortController()
    evalAbort = ctl
    const seq = ++evalSeq
    evaluating.value = true
    try {
      const r = await api.engine.evaluate(arch.value, projectId.value, lang.value, ctl.signal)
      if (seq !== evalSeq) return
      report.value = r
      evalError.value = ''
    } catch (e: any) {
      if (e?.name === 'AbortError' || seq !== evalSeq) return
      evalError.value = errorMessage(e)
    } finally {
      if (seq === evalSeq) evaluating.value = false
    }
  }

  // ----------------------------------------------------------------------------------- simulation
  let simTimer: ReturnType<typeof setTimeout> | undefined
  let simSeq = 0

  async function runScenario(scenario: Scenario) {
    pickingNode.value = false
    if (scenario.type === 'none') { sim.value = null; return }
    sim.value = { scenario, result: sim.value?.result ?? null, loading: true, error: '' }
    const seq = ++simSeq
    try {
      const result = await api.engine.simulate(shown.value, scenario)
      if (seq !== simSeq || !sim.value) return
      sim.value = { scenario, result, loading: false, error: '' }
    } catch (e) {
      if (seq !== simSeq || !sim.value) return
      sim.value = { scenario, result: null, loading: false, error: errorMessage(e) }
    }
  }

  function exitSim() {
    simSeq++
    sim.value = null
    pickingNode.value = false
  }

  function scheduleSim() {
    if (!sim.value) return
    clearTimeout(simTimer)
    const sc = sim.value.scenario
    simTimer = setTimeout(() => void runScenario(sc), app.editorCfg.simulateMs)
  }

  // ------------------------------------------------------------------------------------- history
  function commit(next: Architecture, o: { coalesce?: string } = {}) {
    toasts.clearActions() // "Undo" of the previous action is stale now
    const now = Date.now()
    const merge = !!o.coalesce && o.coalesce === lastKey && now - lastAt < 1200
    if (!merge) {
      past.push(arch.value)
      if (past.length > app.editorCfg.historySteps) past.shift()
    }
    lastKey = o.coalesce ?? ''
    lastAt = now
    future.length = 0
    historyVersion.value++
    arch.value = next
    afterChange()
  }

  function afterChange() {
    touchSave()
    scheduleEvaluate()
    scheduleSim()
  }

  function edit(fn: (draft: Architecture) => void, o: { coalesce?: string } = {}): boolean {
    if (readonly.value) return false
    const next = clone(arch.value)
    fn(next)
    commit(next, o)
    return true
  }

  function undo() {
    const prev = past.pop()
    if (!prev || readonly.value) return
    toasts.clearActions()
    future.push(arch.value)
    arch.value = prev
    lastKey = ''
    historyVersion.value++
    pruneSelection()
    afterChange()
  }

  function redo() {
    const next = future.pop()
    if (!next || readonly.value) return
    toasts.clearActions()
    past.push(arch.value)
    arch.value = next
    lastKey = ''
    historyVersion.value++
    pruneSelection()
    afterChange()
  }

  function pruneSelection() {
    const s = selection.value
    if (!s) return
    const ok = s.type === 'node' ? arch.value.nodes.some((n) => n.id === s.id) : s.type === 'edge' ? arch.value.edges.some((e) => e.id === s.id) : arch.value.zones.some((z) => z.id === s.id)
    if (!ok) selection.value = null
  }

  // --------------------------------------------------------------------------------------- limits
  function underLimit(kind: 'nodes' | 'edges' | 'zones'): boolean {
    const lim = app.boot?.limits
    const max = kind === 'nodes' ? lim?.maxNodes : kind === 'edges' ? lim?.maxEdges : lim?.maxZones
    if (max && arch.value[kind].length >= max) {
      toasts.error(t('editor.limit', { n: max }))
      return false
    }
    return true
  }

  // ------------------------------------------------------------------------------------ mutations
  function uniqueName(base: string): string {
    const names = new Set(arch.value.nodes.map((n) => n.name))
    if (!names.has(base)) return base
    let i = 2
    while (names.has(`${base} ${i}`)) i++
    return `${base} ${i}`
  }

  function addNode(kindId: string, at?: { x: number; y: number }, zoneId?: string | null, props?: Props): string | null {
    const ix = app.index
    const kind = ix?.kinds.get(kindId)
    if (!ix || !kind || readonly.value || !underLimit('nodes')) return null
    const center = at ?? viewCenter.value
    const zone = zoneId !== undefined ? arch.value.zones.find((z) => z.id === zoneId) : zoneAt(arch.value, center.x, center.y)
    const id = newId(kindId.replace(/_/g, '-'))
    const node: ArchNode = {
      id, kind: kindId, name: uniqueName(lt(kind.name)), zoneId: zone?.id ?? null, group: null, role: null, props: { ...(props ?? {}) },
      ui: { x: Math.round(center.x - CARD_W / 2), y: Math.round(center.y - CARD_H / 2) },
    }
    edit((a) => { a.nodes.push(node) })
    selection.value = { type: 'node', id }
    return id
  }

  function addZone(typeId: string, rect?: { x: number; y: number; w: number; h: number }): string | null {
    const ix = app.index
    const zt = ix?.zoneTypes.get(typeId)
    if (!ix || !zt || readonly.value || !underLimit('zones')) return null
    const c = viewCenter.value
    const r = rect ?? { x: c.x - DEFAULT_ZONE_W / 2, y: c.y - DEFAULT_ZONE_H / 2, w: DEFAULT_ZONE_W, h: DEFAULT_ZONE_H }
    const id = newId(typeId)
    const n = arch.value.zones.filter((z) => z.type === typeId).length + 1
    const parent = zoneAt(arch.value, r.x + r.w / 2, r.y + r.h / 2)
    const zone: Zone = {
      id, name: `${lt(zt.name)} ${n}`, type: typeId, parentId: parent && zoneRect(parent) && r.w < zoneRect(parent)!.w ? parent.id : null, role: null, props: {},
      ui: { x: Math.round(r.x), y: Math.round(r.y), w: Math.round(r.w), h: Math.round(r.h) },
    }
    edit((a) => {
      a.zones.push(zone)
      // nodes that were drawn inside the new rectangle move into it
      for (const nd of a.nodes) {
        const nr = nodeRect(nd)
        if (nr && nr.x >= r.x && nr.y >= r.y && nr.x + nr.w <= r.x + r.w && nr.y + nr.h <= r.y + r.h && (!nd.zoneId || nd.zoneId === zone.parentId)) nd.zoneId = id
      }
    })
    selection.value = { type: 'zone', id }
    return id
  }

  function updateNode(id: string, patch: { name?: string; kind?: string; zoneId?: string | null; group?: string | null; role?: string | null }, coalesce?: string) {
    edit((a) => {
      const n = a.nodes.find((x) => x.id === id)
      if (!n) return
      if (patch.name !== undefined) n.name = patch.name
      if (patch.kind !== undefined) n.kind = patch.kind
      if (patch.zoneId !== undefined) n.zoneId = patch.zoneId || null
      if (patch.group !== undefined) n.group = patch.group?.trim() ? patch.group.trim() : null
      if (patch.role !== undefined) n.role = patch.role || null
    }, { coalesce: coalesce ?? `node:${id}:${Object.keys(patch).join(',')}` })
  }

  /**
   * Replace the product of a node (PostgreSQL → MySQL) as ONE step of the history: what still fits stays (sizing, notes), what
   * belonged to the old product (its variant, version, failover settings…) goes. Returns the keys that were dropped.
   */
  function changeKind(id: string, kindId: string): string[] {
    const ix = app.index
    if (!ix || !ix.kinds.has(kindId)) return []
    const dropped: string[] = []
    edit((a) => {
      const n = a.nodes.find((x) => x.id === id)
      if (!n || n.kind === kindId) return
      const oldDefs = new Set(kindPropDefs(ix, n.kind).map((p) => p.key))
      const common = new Set(ix.catalog.commonProps.map((p) => p.key))
      const known = new Set(kindPropDefs(ix, kindId).map((p) => p.key))
      n.kind = kindId
      n.role = null
      for (const key of Object.keys(n.props)) {
        const own = oldDefs.has(key) && !common.has(key)
        // the version and the variant mean something only for the product they were chosen for
        if (key === 'variant' || key === 'version' || (own && !known.has(key))) {
          delete n.props[key]
          if (key !== 'variant') dropped.push(key)
        }
      }
    }, { coalesce: `node:${id}:kind` })
    return dropped
  }
  function setProp(type: 'node' | 'edge' | 'zone', id: string, key: string, value: Scalar | undefined) {
    edit((a) => {
      const target: { props: Props } | undefined = type === 'node' ? a.nodes.find((x) => x.id === id) : type === 'edge' ? a.edges.find((x) => x.id === id) : a.zones.find((x) => x.id === id)
      if (!target) return
      if (value === undefined || value === null || value === '') delete target.props[key]
      else target.props[key] = value
    }, { coalesce: `${type}:${id}:prop:${key}` })
  }

  function updateEdge(id: string, patch: { type?: string; label?: string | null; source?: string; target?: string }) {
    edit((a) => {
      const e = a.edges.find((x) => x.id === id)
      if (!e) return
      if (patch.type !== undefined) e.type = patch.type
      if (patch.label !== undefined) e.label = patch.label?.trim() ? patch.label : null
      if (patch.source !== undefined) e.source = patch.source
      if (patch.target !== undefined) e.target = patch.target
    }, { coalesce: `edge:${id}:${Object.keys(patch).join(',')}` })
  }

  function reverseEdge(id: string) {
    const e = arch.value.edges.find((x) => x.id === id)
    if (e) updateEdge(id, { source: e.target, target: e.source })
  }

  function updateZone(id: string, patch: { name?: string; type?: string; parentId?: string | null; role?: string | null }) {
    edit((a) => {
      const z = a.zones.find((x) => x.id === id)
      if (!z) return
      if (patch.name !== undefined) z.name = patch.name
      if (patch.type !== undefined) z.type = patch.type
      if (patch.parentId !== undefined) z.parentId = patch.parentId && patch.parentId !== id && !zoneChain(a, patch.parentId).includes(id) ? patch.parentId : null
      if (patch.role !== undefined) z.role = patch.role || null
    }, { coalesce: `zone:${id}:${Object.keys(patch).join(',')}` })
  }

  /** Pick the edge type for a new connection from the catalog (semantics and traits), not from ids. */
  function guessEdgeType(a: ArchNode, b: ArchNode): string {
    const ix = app.index!
    const types = [...ix.edgeTypes.values()]
    const of = (semantics: string, pred: (e: (typeof types)[number]) => boolean = () => true) => types.find((e) => e.semantics === semantics && pred(e))?.id
    const ka = ix.kinds.get(a.kind)
    const kb = ix.kinds.get(b.kind)
    const same = !!ka && ka.id === kb?.id
    const guess =
      (kb?.traits.includes('backup-target') && of('backup')) ||
      (same && ka!.behavior === 'consensus' && of('membership')) ||
      (same && ka!.behavior === 'primary_replica' && (of('replication', (e) => e.defaultProps.mode === 'sync') ?? of('replication'))) ||
      ((ka?.traits.includes('monitoring') || ka?.traits.includes('gitops')) && of('control')) ||
      of('dependency')
    return guess || types[0]?.id || 'traffic'
  }

  function connect(source: string, target: string, type?: string): string | null {
    if (source === target || readonly.value || !underLimit('edges')) return null
    const a = arch.value.nodes.find((n) => n.id === source)
    const b = arch.value.nodes.find((n) => n.id === target)
    if (!a || !b) return null
    const ty = type ?? guessEdgeType(a, b)
    const dup = arch.value.edges.find((e) => e.source === source && e.target === target && e.type === ty)
    if (dup) { selection.value = { type: 'edge', id: dup.id }; return dup.id }
    const taken = new Set(arch.value.edges.map((e) => e.id))
    let id = `e-${source}--${target}`
    for (let i = 2; taken.has(id); i++) id = `e-${source}--${target}-${i}`
    const props: Props = {}
    const edge: Edge = { id, source, target, type: ty, label: null, props }
    edit((x) => { x.edges.push(edge) })
    selection.value = { type: 'edge', id }
    return id
  }

  /** A deletion says what went and offers the way back (the toast is gone with the next edit). */
  const announceRemoval = (text: string) => toasts.undoable(text, () => undo())

  function removeNode(id: string) {
    const name = arch.value.nodes.find((n) => n.id === id)?.name
    if (!edit((a) => {
      a.nodes = a.nodes.filter((n) => n.id !== id)
      a.edges = a.edges.filter((e) => e.source !== id && e.target !== id)
    })) return
    if (selection.value?.id === id) selection.value = null
    if (name) announceRemoval(t('toast.removed.node', { name }))
  }

  function removeEdge(id: string) {
    if (!edit((a) => { a.edges = a.edges.filter((e) => e.id !== id) })) return
    if (selection.value?.id === id) selection.value = null
    announceRemoval(t('toast.removed.edge'))
  }

  /** Remove a zone; its nodes and child zones move to the parent zone (or become global). */
  function removeZone(id: string) {
    const name = arch.value.zones.find((z) => z.id === id)?.name
    if (!edit((a) => {
      const z = a.zones.find((x) => x.id === id)
      if (!z) return
      for (const n of a.nodes) if (n.zoneId === id) n.zoneId = z.parentId
      for (const c of a.zones) if (c.parentId === id) c.parentId = z.parentId
      a.zones = a.zones.filter((x) => x.id !== id)
    })) return
    if (selection.value?.id === id) selection.value = null
    if (name) announceRemoval(t('toast.removed.zone', { name }))
  }

  function removeSelection() {
    const s = selection.value
    if (!s || readonly.value) return
    if (s.type === 'node') removeNode(s.id)
    else if (s.type === 'edge') removeEdge(s.id)
    else removeZone(s.id)
  }

  function duplicateNode(id: string) {
    const n = arch.value.nodes.find((x) => x.id === id)
    if (!n || readonly.value || !underLimit('nodes')) return
    const copy: ArchNode = { ...clone(n), id: newId(n.kind.replace(/_/g, '-')), name: uniqueName(n.name), group: null, proposed: undefined, ui: { x: (n.ui?.x ?? 0) + 28, y: (n.ui?.y ?? 0) + 28 } }
    edit((a) => { a.nodes.push(copy) })
    selection.value = { type: 'node', id: copy.id }
  }

  // ------------------------------------------------------------------------------------ bulk actions
  /** Followers of the canvas selection: only existing nodes count, one node is just a selection. */
  function setMulti(ids: string[]) {
    const have = new Set(arch.value.nodes.map((n) => n.id))
    const valid = [...new Set(ids)].filter((id) => have.has(id))
    multi.value = valid.length >= 2 ? valid : []
    if (multi.value.length) selection.value = null
  }

  /** Select nodes (the canvas shows them as selected); one node opens its inspector. */
  function selectNodes(ids: string[]) {
    selectRequest.value = { ids, token: Date.now() }
    if (ids.length === 1) select({ type: 'node', id: ids[0] })
    else setMulti(ids)
  }

  function removeNodes(ids: string[]) {
    const set = new Set(ids)
    if (!set.size || readonly.value) return
    const n = arch.value.nodes.filter((x) => set.has(x.id)).length
    edit((a) => {
      a.nodes = a.nodes.filter((n) => !set.has(n.id))
      a.edges = a.edges.filter((e) => !set.has(e.source) && !set.has(e.target))
    })
    multi.value = []
    if (selection.value && set.has(selection.value.id)) selection.value = null
    if (n) announceRemoval(t('toast.removed.nodes', { n }))
  }

  /** Copies of the nodes and of the edges between them. A group that is copied as a whole becomes a new group. */
  function duplicateNodes(ids: string[]): string[] {
    const set = new Set(ids)
    const src = arch.value.nodes.filter((n) => set.has(n.id))
    if (!src.length || readonly.value) return []
    const max = app.boot?.limits.maxNodes
    if (max && arch.value.nodes.length + src.length > max) { toasts.error(t('editor.limit', { n: max })); return [] }
    const names = new Set(arch.value.nodes.map((n) => n.name))
    const groups = new Set(arch.value.nodes.map((n) => n.group).filter((g): g is string => !!g))
    const fresh = (base: string, taken: Set<string>) => { let i = 2; let c = `${base} ${i}`; while (taken.has(c)) c = `${base} ${++i}`; return c }
    const groupCopy = new Map<string, string>()
    for (const g of new Set(src.map((n) => n.group).filter((x): x is string => !!x))) {
      if (src.filter((n) => n.group === g).length < 2) continue
      let c = `${g}-copy`
      for (let i = 2; groups.has(c); i++) c = `${g}-copy${i}`
      groups.add(c)
      groupCopy.set(g, c)
    }
    const idMap = new Map<string, string>()
    const copies = src.map((n): ArchNode => {
      const id = newId(n.kind.replace(/_/g, '-'))
      idMap.set(n.id, id)
      const name = names.has(n.name) ? fresh(n.name, names) : n.name
      names.add(name)
      return { ...clone(n), id, name, group: (n.group && groupCopy.get(n.group)) || null, proposed: undefined, ui: { ...n.ui, x: (n.ui?.x ?? 0) + 28, y: (n.ui?.y ?? 0) + 28 } }
    })
    const edges = arch.value.edges.filter((e) => idMap.has(e.source) && idMap.has(e.target)).map((e): Edge => ({ ...clone(e), id: `e-${idMap.get(e.source)}--${idMap.get(e.target)}-${e.type}`, source: idMap.get(e.source)!, target: idMap.get(e.target)! }))
    edit((a) => { a.nodes.push(...copies); a.edges.push(...edges) })
    const created = copies.map((c) => c.id)
    selectNodes(created)
    return created
  }

  /** Put the nodes into one group (service / cluster); an empty name dissolves the group. */
  function groupNodes(ids: string[], name: string) {
    const set = new Set(ids)
    const g = name.trim()
    edit((a) => { for (const n of a.nodes) if (set.has(n.id)) n.group = g || null })
  }

  /** Move the nodes into a zone (or out of every zone): they are placed on free slots inside it. */
  function moveNodesToZone(ids: string[], zoneId: string | null) {
    if (readonly.value || !ids.length) return
    if (zoneId === null) {
      const set = new Set(ids)
      edit((a) => { for (const n of a.nodes) if (set.has(n.id)) n.zoneId = null })
      return
    }
    commit(placeInZone(arch.value, ids, zoneId))
  }

  /** After a drag: new absolute positions for nodes/zones (canvas coordinates). */
  function commitMoves(moves: { nodes: Record<string, { x: number; y: number }>; zones: Record<string, { x: number; y: number }> }) {
    if (readonly.value) return
    let movedInto: string | null = null
    edit((a) => {
      const done = new Set<string>()
      for (const [zid, pos] of Object.entries(moves.zones)) {
        const z = a.zones.find((x) => x.id === zid)
        if (!z?.ui) continue
        const dx = Math.round(pos.x - (z.ui.x ?? 0))
        const dy = Math.round(pos.y - (z.ui.y ?? 0))
        if (!dx && !dy) continue
        const d = zoneDescendants(a, zid)
        for (const zz of a.zones) if (d.zones.has(zz.id) && zz.ui) { zz.ui.x = (zz.ui.x ?? 0) + dx; zz.ui.y = (zz.ui.y ?? 0) + dy }
        for (const n of a.nodes) if (d.nodes.has(n.id) && n.ui) { n.ui.x = (n.ui.x ?? 0) + dx; n.ui.y = (n.ui.y ?? 0) + dy; done.add(n.id) }
      }
      for (const [nid, pos] of Object.entries(moves.nodes)) {
        if (done.has(nid)) continue
        const n = a.nodes.find((x) => x.id === nid)
        if (!n) continue
        n.ui = { ...n.ui, x: Math.round(pos.x), y: Math.round(pos.y) }
        const zone = zoneAt(a, pos.x + CARD_W / 2, pos.y + CARD_H / 2)
        const zid = zone?.id ?? null
        if (zid !== n.zoneId) { n.zoneId = zid; movedInto = zone?.name ?? t('inspector.global') }
      }
      // moved zones may have been dropped into another zone
      for (const zid of Object.keys(moves.zones)) {
        const z = a.zones.find((x) => x.id === zid)
        const r = z && zoneRect(z)
        if (!z || !r) continue
        const d = zoneDescendants(a, zid)
        const host = zoneAt(a, r.x + r.w / 2, r.y + 20, d.zones)
        if (host && zoneRect(host) && r.w < zoneRect(host)!.w && z.parentId !== host.id) z.parentId = host.id
        else if (!host && z.parentId && !zoneAt(a, r.x + r.w / 2, r.y + 20, d.zones)) z.parentId = null
      }
    })
    if (movedInto) toasts.info(t('canvas.movedTo', { name: movedInto }))
  }

  function resizeZone(id: string, w: number, h: number) {
    edit((a) => {
      const z = a.zones.find((x) => x.id === id)
      if (z) z.ui = { ...z.ui, w: Math.round(Math.max(240, w)), h: Math.round(Math.max(160, h)) }
    }, { coalesce: `zone:${id}:resize` })
  }

  function setRequirement<K extends keyof Requirements>(key: K, value: Requirements[K] | undefined) {
    edit((a) => {
      const r = a.requirements as Record<string, unknown>
      if (value === undefined || value === null || value === '' || (Array.isArray(value) && !value.length)) delete r[key]
      else r[key] = value
    }, { coalesce: `req:${key}` })
  }

  function setMeta(name: string) {
    edit((a) => { a.meta = { ...a.meta, name } }, { coalesce: 'meta:name' })
  }

  /** Re-run the automatic layout for everything. */
  function autoLayout() {
    if (readonly.value) return
    commit(layoutAll(arch.value, app.index!, layoutOpts()))
    fitToken.value++
  }

  /** Replace the whole architecture (template, restored version, applied AI proposal). */
  function replaceArchitecture(next: Architecture, o: { keepRev?: number; history?: boolean } = {}) {
    const placed = placeMissing(next, app.index!, layoutOpts())
    if (o.history === false) {
      past.length = 0
      future.length = 0
      historyVersion.value++
      arch.value = placed
      afterChange()
    } else commit(placed)
    selection.value = null
    fitToken.value++
    if (o.keepRev !== undefined) {
      rev.value = o.keepRev
      saveState.value = placed === next ? 'saved' : 'dirty'
      if (saveState.value === 'dirty') touchSave()
    }
  }

  function addTemplate(tpl: Architecture, mode: 'replace' | 'add') {
    if (readonly.value) return
    if (mode === 'replace' || isEmpty.value) {
      const merged = clone(tpl)
      merged.requirements = { ...tpl.requirements }
      replaceArchitecture(merged)
    } else {
      commit(mergeArchitecture(arch.value, tpl, app.index!, layoutOpts()))
      fitToken.value++
    }
  }

  /** Apply a finding's fix (a list of patch operations computed by the engine). */
  async function applyFix(f: Finding, fixIndex = 0): Promise<boolean> {
    const fix = f.fixes[fixIndex]
    if (!fix || readonly.value) return false
    return applyOps(fix.ops)
  }

  async function applyOps(ops: Op[]): Promise<boolean> {
    try {
      const r = await api.engine.apply(arch.value, ops, { projectId: projectId.value, lang: lang.value })
      commit(placeMissing(r.architecture, app.index!, layoutOpts()))
      return true
    } catch (e) {
      toasts.error(errorMessage(e))
      return false
    }
  }

  // ------------------------------------------------------------------------------------- waivers
  async function acceptRisk(f: Finding, reason: string, days?: number) {
    // the server validates the fingerprint against the saved draft, so unsaved edits must be stored first
    await flushSave()
    await api.projects.createWaiver(projectId.value, { fingerprint: f.id, reason, expiresInDays: days, lang: lang.value })
    const d = await api.projects.get(projectId.value)
    waivers.value = d.waivers
    await evaluateNow()
  }

  async function revokeWaiver(fingerprint: string) {
    const w = waivers.value.find((x) => x.fingerprint === fingerprint)
    if (!w) return
    await api.projects.revokeWaiver(projectId.value, w.id)
    waivers.value = waivers.value.filter((x) => x.id !== w.id)
    await evaluateNow()
  }

  // ---------------------------------------------------------------------------------- selection
  function select(s: Selection) {
    selection.value = s
    highlight.value = null
    // the canvas keeps the flow's own selection (what a drag moves) in step: one node, or none
    selectRequest.value = { ids: s?.type === 'node' ? [s.id] : [], token: Date.now() }
  }

  function showSubjects(f: { subjects: { nodes: string[]; edges: string[]; zones: string[] } }) {
    highlight.value = { ...f.subjects, token: Date.now() }
    selection.value = null
    setTimeout(() => { if (highlight.value && Date.now() - highlight.value.token >= app.editorCfg.highlightMs) highlight.value = null }, app.editorCfg.highlightMs + 100)
  }

  /** Pan/zoom the canvas to the given elements. */
  function revealElements(nodes: string[], zones: string[] = []) {
    if (nodes.length || zones.length) reveal.value = { nodes, zones, token: Date.now() }
  }

  /** Bring the selected element into view (an edge: both of its ends). */
  function revealSelection() {
    const s = selection.value
    if (!s) return
    if (s.type === 'node') revealElements([s.id])
    else if (s.type === 'zone') revealElements([], [s.id])
    else {
      const e = edgeById.value.get(s.id)
      if (e) revealElements([e.source, e.target])
    }
  }

  function setTool(next: Tool) {
    tool.value = next
    connectFrom.value = null
    pickingNode.value = false
  }

  /** Click on a card: pick a failure target, finish a connection, or just select. */
  function clickNode(id: string, e?: MouseEvent) {
    if (e?.shiftKey) return // Shift+click extends the canvas selection (see `setMulti`)
    if (pickingNode.value) { void runScenario({ type: 'node', nodeId: id }); return }
    if (tool.value === 'connect' && !readonly.value) {
      if (!connectFrom.value) { connectFrom.value = id; selection.value = { type: 'node', id }; return }
      if (connectFrom.value !== id) connect(connectFrom.value, id)
      connectFrom.value = null
      return
    }
    select({ type: 'node', id })
  }

  function setPreview(p: Preview | null) {
    preview.value = p
    if (p) { selection.value = null; fitToken.value++ }
  }

  function boundsOfShown() {
    return boundsOf(shown.value)
  }

  /** Effective (defaults + declared) props of a node, as the engine sees them. */
  function effective(n: ArchNode): Props {
    return effectiveProps(app.index!, n.kind, n.props)
  }

  return {
    projectId, project, arch, rev, waivers, loading, loadError, saveState, saveError, serverRev, report, evaluating, evalError,
    selection, multi, selectRequest, tool, layer, highlight, preview, viewCenter, fitToken, reveal, sim, pickingNode, connectFrom,
    readonly, canUndo, canRedo, isEmpty, shown, previewSets, nodeById, edgeById, zoneById, openFindings, nodeStatus, edgeStatus, zoneStatus, findingsOf, groups, siteZones, rackZones,
    load, reset, flushSave, touchSave, reloadFromServer, evaluateNow, scheduleEvaluate,
    runScenario, exitSim, undo, redo, edit, commit,
    addNode, addZone, updateNode, changeKind, setProp, updateEdge, reverseEdge, updateZone, connect, removeNode, removeEdge, removeZone, removeSelection, duplicateNode,
    setMulti, selectNodes, removeNodes, duplicateNodes, groupNodes, moveNodesToZone, commitMoves, resizeZone, setRequirement, setMeta, autoLayout, replaceArchitecture, addTemplate, applyFix, applyOps,
    acceptRisk, revokeWaiver, select, setTool, clickNode, showSubjects, revealElements, revealSelection, setPreview, boundsOfShown, effective,
  }
})
