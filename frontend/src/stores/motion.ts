import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { FALLBACK_MOTION, parsePref, resolveMode, scaleFactor, type MotionPref } from '@/lib/motion'

const KEY = 'ia_motion'

function storedPref(): MotionPref | null {
  try { return parsePref(localStorage.getItem(KEY)) } catch { return null }
}

/**
 * The motion preference: the user's choice (View menu, kept in this browser), else the administrator's default
 * (`ui.motion`, `ui.motionScale`). Writes the result to `<html data-motion>` and `--motion-scale`.
 */
export const useMotion = defineStore('motion', () => {
  const user = ref<MotionPref | null>(storedPref())
  const adminDefault = ref<MotionPref>(FALLBACK_MOTION.pref)
  const percent = ref<number>(FALLBACK_MOTION.scale)
  const query = typeof window !== 'undefined' && typeof window.matchMedia === 'function' ? window.matchMedia('(prefers-reduced-motion: reduce)') : null
  const systemReduces = ref<boolean>(query?.matches ?? false)
  query?.addEventListener?.('change', (e) => { systemReduces.value = e.matches; apply() })

  const mode = computed(() => resolveMode(user.value, adminDefault.value, systemReduces.value))
  /** Does JavaScript animate (tweened numbers, playback)? Only in `full`: elsewhere values simply arrive. */
  const full = computed(() => mode.value === 'full')
  const factor = computed(() => scaleFactor(percent.value))
  /** The preference as shown in the menu: the user's own, else `system`-or-the-default's name. */
  const choice = computed<MotionPref>(() => user.value ?? adminDefault.value)

  function apply() {
    if (typeof document === 'undefined') return
    const root = document.documentElement
    root.dataset.motion = mode.value
    root.style.setProperty('--motion-scale', String(factor.value))
  }

  /** The administrator's default and speed (from `/public/config` or `/bootstrap`). */
  function configure(def: unknown, scalePercent: unknown) {
    adminDefault.value = parsePref(def) ?? FALLBACK_MOTION.pref
    percent.value = typeof scalePercent === 'number' ? scalePercent : FALLBACK_MOTION.scale
    apply()
  }

  /** The user's own choice; `null` goes back to the administrator's default. */
  function choose(pref: MotionPref | null) {
    user.value = pref
    try { if (pref) localStorage.setItem(KEY, pref); else localStorage.removeItem(KEY) } catch { /* private mode: the choice lasts for this tab */ }
    apply()
  }

  apply()
  return { user, mode, full, factor, choice, configure, choose, apply }
})
