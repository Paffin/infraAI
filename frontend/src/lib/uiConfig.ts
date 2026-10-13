import type { EditorSettings } from '@/api/types'

/**
 * The editor's timings and thresholds are settings (`editor.*`, admin panel → Editor) delivered by `/bootstrap`.
 * These values are only the fallback for a backend that does not send the block yet (a rolling update);
 * `tests/ui-config.test.ts` keeps them equal to the defaults of the settings registry.
 */
export const FALLBACK_EDITOR_SETTINGS: EditorSettings = {
  autosaveMs: 900,
  evaluateMs: 350,
  simulateMs: 450,
  recommendMs: 700,
  historySteps: 100,
  saveRetryMs: 3000,
  saveRetryMaxMs: 30000,
  jobPollMs: 1000,
  lodCompactBelow: 0.62,
  lodMiniBelow: 0.3,
  toastMs: 3500,
  errorToastMs: 7000,
  undoToastMs: 7000,
  highlightMs: 5000,
  paletteMaxResults: 60,
  layoutZonesPerRow: 5,
  quietEdgesSites: 4,
  playbackStepMs: 1600,
  failureLeadMs: 2400,
  flowSpeed: 120,
  flowSpacing: 56,
  flowParticlesMax: 480,
}

/** Settings from the server, with the fallback for whatever it did not send. */
export function editorSettings(sent?: Partial<EditorSettings> | null): EditorSettings {
  const out = { ...FALLBACK_EDITOR_SETTINGS }
  for (const k of Object.keys(out) as (keyof EditorSettings)[]) {
    const v = sent?.[k]
    if (typeof v === 'number' && Number.isFinite(v)) out[k] = v
  }
  return out
}
