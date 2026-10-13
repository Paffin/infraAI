import { defineStore } from 'pinia'
import { computed, ref, watch } from 'vue'
import type { Need } from '@/api/types'
import { useAi } from './ai'
import { useApp } from './app'

/** `sim`: the failure that is on the diagram (its story, the traffic); the tab is there only while a failure is shown. */
export type RightTab = 'checks' | 'requirements' | 'analysis' | 'chat' | 'sim'
/** What the analysis tab shows: the numbers about the design (what it carries, how often it is up, what it costs) and where it runs. */
export type AnalysisView = 'capacity' | 'availability' | 'cost' | 'sites'
export const ANALYSIS_VIEWS: AnalysisView[] = ['capacity', 'availability', 'cost', 'sites']
export type ModalKind = 'versions' | 'export' | 'matrix' | 'score' | 'shortcuts' | 'placement' | null

const read = (k: string): string | null => { try { return localStorage.getItem(k) } catch { return null } }
const write = (k: string, v: string) => { try { localStorage.setItem(k, v) } catch { /* ignore */ } }

/** View state of the editor shell that does not belong to the project itself. */
export const useUi = defineStore('ui', () => {
  const ai = useAi()
  const aiMode = ref<'ai' | 'manual'>(read('ia_ai_mode') === 'manual' ? 'manual' : 'ai')
  const tab = ref<RightTab>('checks')
  const storedView = read('ia_analysis')
  const analysis = ref<AnalysisView>(ANALYSIS_VIEWS.includes(storedView as AnalysisView) ? (storedView as AnalysisView) : 'capacity')
  const modal = ref<ModalKind>(null)
  const palette = ref(false)
  /** Bumped to ask the prompt bar for the keyboard focus (the empty canvas offers "describe it"). */
  const promptFocus = ref(0)
  /** A field the user was sent to ("fill in the RTO"): the inspector / requirements form opens, focuses and flashes it once. */
  const focusRequest = ref<{ need: Need; token: number } | null>(null)
  // on small screens the library starts collapsed (an explicit choice is remembered)
  const stored = read('ia_library')
  const libraryOpen = ref(stored ? stored === 'open' : window.innerWidth >= 1500)

  // ---- the layout of the panels (kept per browser): the right panel folds to a rail of its tabs and is dragged to the width the user wants;
  // the what-if bar and the failure panel fold, and float where the user puts them (lib/useFloat.ts keeps their positions)
  const app = useApp()
  const rightOpen = ref(read('ia_right') !== 'closed')
  const storedWidth = Number(read('ia_right_w'))
  const rightWidthOwn = ref<number | null>(Number.isFinite(storedWidth) && storedWidth > 0 ? storedWidth : null)
  /** The width of the right panel in px: the user's own, or the default of the settings, always between the limits of the settings. */
  const rightWidth = computed(() => {
    const c = app.editorCfg
    return Math.min(c.rightPanelMaxWidth, Math.max(c.rightPanelMinWidth, rightWidthOwn.value ?? c.rightPanelWidth))
  })
  /** Set the width the user dragged to; null goes back to the default. */
  function setRightWidth(px: number | null) {
    rightWidthOwn.value = px
    write('ia_right_w', px === null ? '' : String(Math.round(px)))
  }
  watch(rightOpen, (v) => write('ia_right', v ? 'open' : 'closed'))
  /** The what-if bar folded to a pill. */
  const whatIfFolded = ref(read('ia_whatif_fold') === '1')
  watch(whatIfFolded, (v) => write('ia_whatif_fold', v ? '1' : '0'))
  /** The failure panel: a tab of the right panel (`dock`) or a window over the diagram (`float`); folded to its header. */
  const simMode = ref<'dock' | 'float'>(read('ia_sim_mode') === 'float' ? 'float' : 'dock')
  watch(simMode, (v) => write('ia_sim_mode', v))
  const simFolded = ref(read('ia_sim_fold') === '1')
  watch(simFolded, (v) => write('ia_sim_fold', v ? '1' : '0'))

  watch(aiMode, (v) => write('ia_ai_mode', v))
  watch(analysis, (v) => write('ia_analysis', v))
  /** Open one of the analyses: the analysis tab, on that view. */
  function showAnalysis(view: AnalysisView) {
    analysis.value = view
    tab.value = 'analysis'
  }
  watch(libraryOpen, (v) => write('ia_library', v ? 'open' : 'closed'))
  /** AI features are usable: the server has a key, the user did not switch to manual mode. */
  const aiOn = computed(() => aiMode.value === 'ai' && ai.enabled)

  return { aiMode, tab, analysis, showAnalysis, modal, palette, promptFocus, focusRequest, libraryOpen, aiOn, rightOpen, rightWidth, setRightWidth, whatIfFolded, simMode, simFolded }
})
