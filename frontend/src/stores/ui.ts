import { defineStore } from 'pinia'
import { computed, ref, watch } from 'vue'
import type { Need } from '@/api/types'
import { useAi } from './ai'

export type RightTab = 'checks' | 'requirements' | 'analysis' | 'chat'
/** What the analysis tab shows: the numbers about the design (what it carries, how often it is up, what it costs). */
export type AnalysisView = 'capacity' | 'availability' | 'cost'
export const ANALYSIS_VIEWS: AnalysisView[] = ['capacity', 'availability', 'cost']
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
  const showSimDetails = ref(true)
  // on small screens the library starts collapsed (an explicit choice is remembered)
  const stored = read('ia_library')
  const libraryOpen = ref(stored ? stored === 'open' : window.innerWidth >= 1500)

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

  return { aiMode, tab, analysis, showAnalysis, modal, palette, promptFocus, focusRequest, showSimDetails, libraryOpen, aiOn }
})
