import { computed, type Component, type ComputedRef } from 'vue'
import { useRouter } from 'vue-router'
import {
  Activity, Boxes, Coins, Copy, Download, Gauge, History, Keyboard, Languages, Layers, LayoutDashboard, ListChecks, Maximize, MousePointer2, PanelLeft, Play, Plus, Redo2, Save, ShieldCheck,
  MapPinned, Network, Sparkles, Spline, Square, Trash2, Undo2, Zap, Folder, Users,
} from 'lucide-vue-next'
import type { KindDef, Status } from '@/api/types'
import { lang, lt, setLang, t } from '@/i18n'
import { findingText, findingTitle, statusColor, statusLabel } from '@/lib/findings'
import { shortcutText } from '@/lib/shortcuts'
import { scenarioLabel } from '@/lib/sim'
import { useApp } from '@/stores/app'
import { useDialogs } from '@/stores/dialogs'
import { useEditor } from '@/stores/editor'
import { ANALYSIS_VIEWS, useUi, type AnalysisView, type RightTab } from '@/stores/ui'

export type CommandGroup = 'element' | 'finding' | 'scenario' | 'add' | 'edit' | 'view' | 'panel'
export const GROUP_ORDER: CommandGroup[] = ['element', 'finding', 'scenario', 'add', 'edit', 'view', 'panel']

export interface Command {
  id: string
  group: CommandGroup
  title: string
  hint?: string
  /** more words that find the command */
  keywords?: string[]
  shortcut?: string
  /** the kind of component the command is about: it is shown by its icon (components) */
  kind?: KindDef
  /** abbreviation on a coloured tile, for a command whose component is not in the catalog */
  badge?: { text: string; color: string }
  icon?: Component
  /** colour of the icon */
  color?: string
  /** status dot at the right edge */
  dot?: string
  /** listed when the query is empty */
  suggest?: boolean
  run: () => void
}

const TABS: RightTab[] = ['checks', 'requirements', 'chat']
const TAB_ICON: Record<RightTab, Component> = { checks: ListChecks, requirements: ShieldCheck, analysis: Gauge, chat: Sparkles, sim: Zap }
const ANALYSIS_ICON: Record<AnalysisView, Component> = { capacity: Gauge, availability: Activity, cost: Coins, sites: MapPinned }

/** Everything the command palette (Ctrl/⌘+K) can find or do for the open project; rebuilt when the project, the report or the language changes. */
export function usePaletteCommands(): ComputedRef<Command[]> {
  const app = useApp()
  const editor = useEditor()
  const ui = useUi()
  const router = useRouter()
  const dialogs = useDialogs()

  return computed<Command[]>(() => {
    void lang.value // the titles depend on the language
    const out: Command[] = []
    const ix = app.index
    if (!ix) return out
    const canEdit = app.canEdit && !editor.readonly
    const report = editor.report
    const multi = editor.multi
    const sel = editor.selection

    // ---------------------------------------------------------------- elements of the diagram
    for (const n of editor.shown.nodes) {
      const kind = ix.kinds.get(n.kind)
      const zone = n.zoneId ? editor.zoneById.get(n.zoneId) : undefined
      const status: Status | undefined = editor.nodeStatus.get(n.id)
      out.push({
        id: 'node:' + n.id, group: 'element', title: n.name,
        hint: [kind ? lt(kind.name) : n.kind, zone?.name].filter(Boolean).join(' · '),
        keywords: [n.kind, n.group ?? '', kind?.abbr ?? ''],
        kind, badge: { text: kind?.abbr ?? '?', color: kind?.color ?? '#4c596c' },
        dot: status ? statusColor(status) : undefined,
        run: () => { editor.select({ type: 'node', id: n.id }); editor.revealSelection() },
      })
    }
    for (const z of editor.shown.zones) {
      const def = ix.zoneTypes.get(z.type)
      const status = editor.zoneStatus.get(z.id)
      out.push({
        id: 'zone:' + z.id, group: 'element', title: z.name, hint: def ? lt(def.name) : z.type, keywords: [z.type, z.role ?? ''],
        icon: Square, color: def?.color, dot: status ? statusColor(status) : undefined,
        run: () => { editor.select({ type: 'zone', id: z.id }); editor.revealSelection() },
      })
    }

    // ---------------------------------------------------------------- findings
    for (const f of editor.openFindings) {
      const rule = app.rules.get(f.ruleId)
      out.push({
        id: 'finding:' + f.id, group: 'finding', title: findingTitle(rule, f), hint: findingText(rule, f),
        keywords: [f.ruleId, statusLabel(f.status)], dot: statusColor(f.status),
        run: () => { ui.tab = 'checks'; editor.showSubjects(f) },
      })
    }

    // ---------------------------------------------------------------- failure scenarios
    const scenarios = report?.scenarios ?? []
    scenarios.forEach((s, i) => {
      out.push({
        id: 'scenario:' + i, group: 'scenario', title: t('cmd.scenario', { name: scenarioLabel(s.scenario, editor.shown, report?.units) }),
        hint: t('sim.result.' + s.state), keywords: ['simulate', 'failure', 'симуляция', 'отказ'], icon: Play, color: s.state === 'down' ? 'var(--crit)' : s.state === 'degraded' ? 'var(--risk-dot)' : 'var(--ok-dot)',
        suggest: i < 4, run: () => void editor.runScenario(s.scenario),
      })
    })
    if (!editor.isEmpty) {
      out.push({ id: 'scenario:node', group: 'scenario', title: t('cmd.sim.node'), keywords: ['simulate', 'failure', 'симуляция', 'отказ'], icon: Zap, suggest: true, run: () => { editor.pickingNode = true } })
    }
    if (editor.sim) out.push({ id: 'scenario:exit', group: 'scenario', title: t('cmd.sim.exit'), icon: Zap, suggest: true, shortcut: 'Esc', run: () => editor.exitSim() })

    // ---------------------------------------------------------------- add components and zones
    if (canEdit) {
      for (const k of ix.catalog.kinds.filter((x) => !x.hidden)) {
        const cat = ix.catalog.categories.find((c) => c.id === k.category)
        out.push({
          id: 'add:' + k.id, group: 'add', title: t('cmd.add', { name: lt(k.name) }), hint: cat ? lt(cat.name) : undefined,
          keywords: [k.id, k.abbr, lt(k.description), k.function ?? '', 'add', 'добавить', 'создать'], kind: k, badge: { text: k.abbr, color: k.color }, run: () => { editor.addNode(k.id) },
        })
        // a way to deploy the product is found by its name too ("Aurora", "Patroni", "Galera"): add the node with it
        for (const v of k.variants ?? []) {
          out.push({
            id: `add:${k.id}:${v.id}`, group: 'add', title: t('cmd.add', { name: `${lt(k.name)} — ${lt(v.name)}` }), hint: cat ? lt(cat.name) : undefined,
            keywords: [v.id, k.id, lt(v.description), 'add', 'добавить', 'создать'], kind: k, badge: { text: k.abbr, color: k.color }, run: () => { editor.addNode(k.id, undefined, undefined, { variant: v.id }) },
          })
        }
      }
      for (const z of ix.catalog.zoneTypes) {
        out.push({ id: 'addzone:' + z.id, group: 'add', title: t('cmd.addzone', { name: lt(z.name) }), keywords: [z.id, 'zone', 'зона', 'add', 'добавить'], icon: Square, color: z.color, run: () => { editor.addZone(z.id) } })
      }
    }

    // ---------------------------------------------------------------- edit
    if (canEdit) {
      out.push({ id: 'edit:undo', group: 'edit', title: t('tool.undo'), icon: Undo2, shortcut: shortcutText('undo'), suggest: editor.canUndo, run: () => editor.undo() })
      out.push({ id: 'edit:redo', group: 'edit', title: t('tool.redo'), icon: Redo2, shortcut: shortcutText('redo'), suggest: editor.canRedo, run: () => editor.redo() })
      out.push({ id: 'edit:layout', group: 'edit', title: t('tool.layout'), icon: LayoutDashboard, keywords: ['layout', 'раскладка'], suggest: true, run: () => editor.autoLayout() })
      out.push({ id: 'edit:placement', group: 'edit', title: t('placement.open'), icon: Network, keywords: ['placement', 'sites', 'racks', 'размещение', 'площадки', 'цод', 'стойки', 'регионы'], suggest: true, run: () => { ui.modal = 'placement' } })
      out.push({ id: 'edit:tool.select', group: 'edit', title: t('cmd.tool.select'), icon: MousePointer2, shortcut: shortcutText('tool.select'), run: () => editor.setTool('select') })
      out.push({ id: 'edit:tool.connect', group: 'edit', title: t('cmd.tool.connect'), icon: Spline, shortcut: shortcutText('tool.connect'), keywords: ['connect', 'edge', 'связь'], run: () => editor.setTool('connect') })
      out.push({ id: 'edit:tool.zone', group: 'edit', title: t('cmd.tool.zone'), icon: Square, shortcut: shortcutText('tool.zone'), keywords: ['zone', 'зона'], run: () => editor.setTool('zone') })
      if (sel?.type === 'node') out.push({ id: 'edit:duplicate', group: 'edit', title: t('cmd.duplicate'), icon: Copy, shortcut: shortcutText('duplicate'), run: () => editor.duplicateNode(sel.id) })
      if (multi.length >= 2) {
        out.push({ id: 'edit:duplicate.multi', group: 'edit', title: t('cmd.duplicate.multi', { n: multi.length }), icon: Copy, shortcut: shortcutText('duplicate'), suggest: true, run: () => { editor.duplicateNodes(multi) } })
        out.push({
          id: 'edit:group', group: 'edit', title: t('cmd.group', { n: multi.length }), icon: Boxes, suggest: true,
          run: () => {
            void dialogs.prompt({ title: t('bulk.group.title'), label: t('bulk.group.label'), value: editor.arch.nodes.find((n) => n.id === multi[0])?.group ?? '' }).then((v) => { if (v !== null) editor.groupNodes(multi, v) })
          },
        })
        out.push({ id: 'edit:delete.multi', group: 'edit', title: t('cmd.delete.multi', { n: multi.length }), icon: Trash2, shortcut: shortcutText('delete'), suggest: true, run: () => editor.removeNodes(multi) })
      } else if (sel) {
        out.push({ id: 'edit:delete', group: 'edit', title: t('cmd.delete'), icon: Trash2, shortcut: shortcutText('delete'), run: () => editor.removeSelection() })
      }
      if (editor.arch.nodes.length) out.push({ id: 'edit:selectAll', group: 'edit', title: t('cmd.selectAll'), shortcut: shortcutText('selectAll'), icon: Plus, run: () => editor.selectNodes(editor.arch.nodes.map((n) => n.id)) })
    }
    out.push({ id: 'edit:save', group: 'edit', title: t('cmd.save'), icon: Save, shortcut: shortcutText('save'), keywords: ['save', 'сохранить'], run: () => void editor.flushSave() })

    // ---------------------------------------------------------------- view
    out.push({ id: 'view:fit', group: 'view', title: t('canvas.fit'), icon: Maximize, keywords: ['fit', 'zoom', 'масштаб', 'вид'], suggest: true, run: () => { editor.fitToken++ } })
    out.push({ id: 'view:library', group: 'view', title: ui.libraryOpen ? t('library.collapse') : t('library.expand'), icon: PanelLeft, shortcut: undefined, run: () => { ui.libraryOpen = !ui.libraryOpen } })
    out.push({ id: 'view:mode', group: 'view', title: ui.aiMode === 'ai' ? t('cmd.mode.manual') : t('cmd.mode.ai'), icon: Sparkles, keywords: ['ai', 'ии', 'manual', 'вручную'], run: () => { ui.aiMode = ui.aiMode === 'ai' ? 'manual' : 'ai' } })
    for (const l of ['all', 'network', 'data', 'dr'] as const) {
      out.push({ id: 'view:layer.' + l, group: 'view', title: t('cmd.layer', { name: t('layer.' + l) }), icon: Layers, keywords: ['layer', 'слой'], run: () => { editor.layer = l } })
    }
    for (const l of app.boot?.app.languages ?? []) {
      if (l !== lang.value) out.push({ id: 'view:lang.' + l, group: 'view', title: t('cmd.lang', { name: l.toUpperCase() }), icon: Languages, keywords: ['language', 'язык'], run: () => setLang(l) })
    }

    // ---------------------------------------------------------------- panels and windows
    for (const tab of TABS) {
      out.push({ id: 'panel:' + tab, group: 'panel', title: t('cmd.open', { name: t('tab.' + tab) }), icon: TAB_ICON[tab], suggest: true, run: () => { ui.tab = tab } })
    }
    for (const view of ANALYSIS_VIEWS) {
      out.push({ id: 'panel:' + view, group: 'panel', title: t('cmd.open', { name: t('tab.' + view) }), icon: ANALYSIS_ICON[view], suggest: true, run: () => ui.showAnalysis(view) })
    }
    out.push({ id: 'panel:matrix', group: 'panel', title: t('editor.sim'), icon: Zap, keywords: ['matrix', 'матрица'], suggest: true, run: () => { ui.modal = 'matrix' } })
    out.push({ id: 'panel:versions', group: 'panel', title: t('editor.versions'), icon: History, keywords: ['versions', 'diff', 'история'], suggest: true, run: () => { ui.modal = 'versions' } })
    out.push({ id: 'panel:export', group: 'panel', title: t('editor.export'), icon: Download, keywords: ['export', 'terraform', 'helm', 'kubernetes', 'iac'], suggest: true, run: () => { ui.modal = 'export' } })
    out.push({ id: 'panel:score', group: 'panel', title: t('editor.score.title'), icon: ShieldCheck, keywords: ['score', 'оценка'], run: () => { ui.modal = 'score' } })
    out.push({ id: 'panel:shortcuts', group: 'panel', title: t('cmd.shortcuts'), icon: Keyboard, shortcut: shortcutText('shortcuts'), suggest: true, run: () => { ui.modal = 'shortcuts' } })
    out.push({ id: 'panel:projects', group: 'panel', title: t('cmd.projects'), icon: Folder, keywords: ['projects', 'проекты'], run: () => { void editor.flushSave().then(() => router.push({ name: 'projects' })) } })
    if (app.isAdmin) out.push({ id: 'panel:admin', group: 'panel', title: t('cmd.admin'), icon: Users, keywords: ['admin', 'settings', 'настройки', 'админка'], run: () => { void editor.flushSave().then(() => router.push({ name: 'admin' })) } })
    return out
  })
}
