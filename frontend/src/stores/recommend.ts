import { defineStore } from 'pinia'
import { computed, ref, shallowRef } from 'vue'
import * as api from '@/api/endpoints'
import type { RecommendPlan } from '@/api/types'
import { lang } from '@/i18n'
import { busyRetryMs } from '@/lib/busy'
import { errorMessage } from '@/lib/format'
import { archFingerprint } from '@/lib/recommend'
import { useApp } from './app'
import { useEditor } from './editor'

/**
 * "What to improve first": the engine's fixes for the open findings, each tried on a copy of the design by the server and ranked
 * by the effect it measurably has (`docs/RECOMMENDATIONS.md`). The plan belongs to the design it was made for: after a real edit it is
 * out of date (its figures and its operations were for the old design) until the next one arrives, and nothing is applied from it.
 * Moving a card is not an edit of the design, so it does not make the plan wait.
 */
export const useRecommend = defineStore('recommend', () => {
  const app = useApp()
  const editor = useEditor()

  const plan = shallowRef<RecommendPlan | null>(null)
  /** The design (project and structure, not positions) the plan was made for. */
  const planKey = ref('')
  const loading = ref(false)
  const error = ref('')

  const cfg = computed(() => app.boot?.engine.recommendations)
  const enabled = computed(() => cfg.value?.enabled === true)
  /** How many are listed before "show more" (`engine.recommendations.show`). */
  const show = computed(() => Math.max(1, cfg.value?.show ?? 4))

  /** The engine has something to try: an open critical / risk finding that carries a fix. */
  const wanted = computed(() => enabled.value && !editor.isEmpty && (editor.report?.findings ?? []).some((f) => !f.accepted && (f.status === 'critical' || f.status === 'risk') && f.fixes.some((x) => x.ops.length)))

  /** What the plan depends on: the project, the language of the labels, the accepted risks and the structure of the design. */
  const key = computed(() => `${editor.projectId}|${lang.value}|${editor.waivers.map((w) => w.fingerprint).sort().join(',')}|${archFingerprint(editor.arch)}`)
  /** Of this design, not of an earlier one (or another project): safe to apply. */
  const fresh = computed(() => plan.value !== null && planKey.value === key.value)
  /** What is on the screen: the plan of this project (out of date or not), never another project's. */
  const current = computed(() => (plan.value && planKey.value.startsWith(editor.projectId + '|') ? plan.value : null))

  let timer: ReturnType<typeof setTimeout> | undefined
  let ctl: AbortController | undefined
  let seq = 0

  function clear() {
    clearTimeout(timer)
    ctl?.abort()
    seq++
    plan.value = null
    planKey.value = ''
    loading.value = false
    error.value = ''
  }

  async function load() {
    ctl?.abort()
    const mine = new AbortController()
    ctl = mine
    const id = ++seq
    const forKey = key.value
    loading.value = true
    let later = false
    try {
      const p = await api.engine.recommendations(editor.arch, editor.projectId || undefined, lang.value, mine.signal)
      if (id !== seq) return
      plan.value = p
      planKey.value = forKey
      error.value = ''
    } catch (e: any) {
      if (e?.name === 'AbortError' || id !== seq) return
      // the server is busy with this person's other calculations (429 + Retry-After): the plan is asked for again when the server says to, and no error is shown meanwhile.
      // The request is the editor's own, after every settled edit; an edit or a clear in the meantime replaces the retry (`ensure`, `clear`)
      const wait = busyRetryMs(e)
      if (wait !== null) {
        later = true
        clearTimeout(timer)
        timer = setTimeout(() => { if (id === seq) void load() }, wait)
        return
      }
      error.value = errorMessage(e)
    } finally {
      if (id === seq && !later) loading.value = false
    }
  }

  /**
   * Called when the checks are on the screen and whenever their report changes: asks for a plan once the design has settled
   * (`editor.recommendMs` after the check), or drops the plan when there is nothing to improve.
   */
  function ensure() {
    const r = editor.report
    clearTimeout(timer)
    if (!r || !wanted.value) {
      if (plan.value || loading.value) clear()
      return
    }
    // a proposal of the AI is being looked at: the working copy is not the subject now
    if (editor.preview) return
    // the plan of another project is not an answer for this one
    if (plan.value && !planKey.value.startsWith(editor.projectId + '|')) clear()
    if (fresh.value) return
    timer = setTimeout(() => void load(), app.editorCfg.recommendMs)
  }

  return { plan, current, fresh, loading, error, enabled, show, wanted, ensure, clear }
})
