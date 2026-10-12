import { defineStore } from 'pinia'
import { computed, ref, watch } from 'vue'
import { useAi } from './ai'

export type RightTab = 'checks' | 'requirements' | 'cost' | 'chat'
export type ModalKind = 'versions' | 'export' | 'matrix' | 'score' | null

const read = (k: string): string | null => { try { return localStorage.getItem(k) } catch { return null } }
const write = (k: string, v: string) => { try { localStorage.setItem(k, v) } catch { /* ignore */ } }

/** View state of the editor shell that does not belong to the project itself. */
export const useUi = defineStore('ui', () => {
  const ai = useAi()
  const aiMode = ref<'ai' | 'manual'>(read('ia_ai_mode') === 'manual' ? 'manual' : 'ai')
  const tab = ref<RightTab>('checks')
  const modal = ref<ModalKind>(null)
  const showSimDetails = ref(true)
  // on small screens the library starts collapsed (an explicit choice is remembered)
  const stored = read('ia_library')
  const libraryOpen = ref(stored ? stored === 'open' : window.innerWidth >= 1500)

  watch(aiMode, (v) => write('ia_ai_mode', v))
  watch(libraryOpen, (v) => write('ia_library', v ? 'open' : 'closed'))
  /** AI features are usable: the server has a key, the user did not switch to manual mode. */
  const aiOn = computed(() => aiMode.value === 'ai' && ai.enabled)

  return { aiMode, tab, modal, showSimDetails, libraryOpen, aiOn }
})
