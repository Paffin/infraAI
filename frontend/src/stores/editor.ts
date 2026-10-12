import { defineStore } from 'pinia'
import { computed, ref, shallowRef } from 'vue'
import { ApiError } from '@/api/client'
import * as api from '@/api/endpoints'
import type {
  ArchDiff, ArchNode, Architecture, Edge, Finding, Op, ProjectDetail, Props, Report, Requirements, Scalar, Scenario, SimResult, Status, Waiver, Zone,
} from '@/api/types'
import { CARD_H, CARD_W, DEFAULT_ZONE_H, DEFAULT_ZONE_W, boundsOf, layoutAll, mergeArchitecture, nodeRect, placeMissing, zoneAt, zoneChain, zoneDescendants, zoneRect } from '@/editor/layout'
import { lang, lt, t } from '@/i18n'
import { effectiveProps } from '@/lib/catalog'
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
const HISTORY_LIMIT = 100
const SAVE_DELAY = 900
const EVAL_DELAY = 350

export const useEditor = defineStore('editor', () => {
  const app = useApp()
  const toasts = useToasts()

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
  const tool = ref<Tool>('select')
  const layer = ref<Layer>('all')
  const highlight = ref<{ nodes: string[]; edges: string[]; zones: string[]; token: number } | null>(null)
  const preview = shallowRef<Preview | null>(null)
  const viewCenter = ref<{ x: number; y: number }>({ x: 400, y: 300 })
  const fitToken = ref(0)

  const sim = ref<{ scenario: Scenario; result: SimResult | null; loading: boolean; error: string } | null>(null)
  const pickingNode = ref(false)
  const connectFrom = ref<string | null>(null)

  const past: Architecture[] = []
  const future: Architecture[] = []
  const historyVersion = ref(0)
  let lastKey = ''
  let lastAt = 0

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
    return placeMissing(out, app.index!)
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
      const placed = placeMissing(d.architecture, app.index!)
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
    tool.value = 'select'
    preview.value = null
    sim.value = null
    pickingNode.value = false
    connectFrom.value = null
    highlight.value = null
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
  let retryDelay = 3000

  function touchSave() {
    if (!app.canEdit || saveState.value === 'conflict') return
    saveState.value = 'dirty'
    clearTimeout(saveTimer)
    saveTimer = setTimeout(() => void flushSave(), SAVE_DELAY)
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
      retryDelay = 3000
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
          saveTimer = setTimeout(() => { saveState.value = 'dirty'; void flushSave() }, retryDelay)
          retryDelay = Math.min(retryDelay * 2, 30000)
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
    evalTimer = setTimeout(() => void evaluateNow(), EVAL_DELAY)
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
    simTimer = setTimeout(() => void runScenario(sc), 450)
  }

  // ------------------------------------------------------------------------------------- history
  function commit(next: Architecture, o: { coalesce?: string } = {}) {
    const now = Date.now()
    const merge = !!o.coalesce && o.coalesce === lastKey && now - lastAt < 1200
    if (!merge) {
      past.push(arch.value)
      if (past.length > HISTORY_LIMIT) past.shift()
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

  function addNode(kindId: string, at?: { x: number; y: number }, zoneId?: string | null): string | null {
    const ix = app.index
    const kind = ix?.kinds.get(kindId)
    if (!ix || !kind || readonly.value || !underLimit('nodes')) return null
    const center = at ?? viewCenter.value
    const zone = zoneId !== undefined ? arch.value.zones.find((z) => z.id === zoneId) : zoneAt(arch.value, center.x, center.y)
    const id = newId(kindId.replace(/_/g, '-'))
    const node: ArchNode = {
      id, kind: kindId, name: uniqueName(lt(kind.name)), zoneId: zone?.id ?? null, group: null, role: null, props: {},
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

  function guessEdgeType(a: ArchNode, b: ArchNode): string {
    const ix = app.index!
    const ka = ix.kinds.get(a.kind)
    const kb = ix.kinds.get(b.kind)
    if (kb?.id === 'backup_storage' || kb?.traits.includes('backup-target')) return 'backup'
    if (ka && kb && ka.id === kb.id && ka.behavior === 'consensus') return 'quorum'
    if (ka && kb && ka.id === kb.id && ka.behavior === 'primary_replica') return 'sync'
    if (ka?.id === 'monitoring' || ka?.id === 'gitops') return 'control'
    return 'traffic'
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

  function removeNode(id: string) {
    edit((a) => {
      a.nodes = a.nodes.filter((n) => n.id !== id)
      a.edges = a.edges.filter((e) => e.source !== id && e.target !== id)
    })
    if (selection.value?.id === id) selection.value = null
  }

  function removeEdge(id: string) {
    edit((a) => { a.edges = a.edges.filter((e) => e.id !== id) })
    if (selection.value?.id === id) selection.value = null
  }

  /** Remove a zone; its nodes and child zones move to the parent zone (or become global). */
  function removeZone(id: string) {
    edit((a) => {
      const z = a.zones.find((x) => x.id === id)
      if (!z) return
      for (const n of a.nodes) if (n.zoneId === id) n.zoneId = z.parentId
      for (const c of a.zones) if (c.parentId === id) c.parentId = z.parentId
      a.zones = a.zones.filter((x) => x.id !== id)
    })
    if (selection.value?.id === id) selection.value = null
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
    commit(layoutAll(arch.value, app.index!))
    fitToken.value++
  }

  /** Replace the whole architecture (template, restored version, applied AI proposal). */
  function replaceArchitecture(next: Architecture, o: { keepRev?: number; history?: boolean } = {}) {
    const placed = placeMissing(next, app.index!)
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
      commit(mergeArchitecture(arch.value, tpl, app.index!))
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
      commit(placeMissing(r.architecture, app.index!))
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
  }

  function showSubjects(f: { subjects: { nodes: string[]; edges: string[]; zones: string[] } }) {
    highlight.value = { ...f.subjects, token: Date.now() }
    selection.value = null
    setTimeout(() => { if (highlight.value && Date.now() - highlight.value.token >= 5000) highlight.value = null }, 5100)
  }

  function setTool(next: Tool) {
    tool.value = next
    connectFrom.value = null
    pickingNode.value = false
  }

  /** Click on a card: pick a failure target, finish a connection, or just select. */
  function clickNode(id: string, _e?: MouseEvent) {
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
    selection, tool, layer, highlight, preview, viewCenter, fitToken, sim, pickingNode, connectFrom,
    readonly, canUndo, canRedo, isEmpty, shown, previewSets, nodeById, edgeById, zoneById, openFindings, nodeStatus, edgeStatus, zoneStatus, findingsOf, groups, siteZones,
    load, reset, flushSave, touchSave, reloadFromServer, evaluateNow, scheduleEvaluate,
    runScenario, exitSim, undo, redo, edit, commit,
    addNode, addZone, updateNode, setProp, updateEdge, reverseEdge, updateZone, connect, removeNode, removeEdge, removeZone, removeSelection, duplicateNode,
    commitMoves, resizeZone, setRequirement, setMeta, autoLayout, replaceArchitecture, addTemplate, applyFix, applyOps,
    acceptRisk, revokeWaiver, select, setTool, clickNode, showSubjects, setPreview, boundsOfShown, effective,
  }
})
