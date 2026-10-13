import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import type { SettingDef } from '../src/api/types'
import { editorSettings, FALLBACK_EDITOR_SETTINGS } from '../src/lib/uiConfig'

const registry: SettingDef[] = JSON.parse(readFileSync(join(process.cwd(), '..', 'backend', 'crates', 'server', 'defaults', 'settings.json'), 'utf8'))
const editorDefs = registry.filter((d) => d.key.startsWith('editor.'))

describe('editor settings', () => {
  it('the fallback is exactly the default of every `editor.*` setting of the registry', () => {
    const defaults = Object.fromEntries(editorDefs.map((d) => [d.key.slice('editor.'.length), d.default]))
    expect(FALLBACK_EDITOR_SETTINGS).toEqual(defaults)
  })

  it('every setting has a range that contains its default and labels in both languages', () => {
    for (const d of editorDefs) {
      expect(d.type, d.key).toBe('number')
      expect(d.min, d.key).toBeDefined()
      expect(d.max, d.key).toBeDefined()
      expect(d.default as number, d.key).toBeGreaterThanOrEqual(d.min!)
      expect(d.default as number, d.key).toBeLessThanOrEqual(d.max!)
      const label = d.label as Record<string, string>
      expect(label.ru?.length, d.key).toBeGreaterThan(3)
      expect(label.en?.length, d.key).toBeGreaterThan(3)
      expect(d.group).toBe('editor')
    }
  })

  it('the thresholds of the semantic zoom are ordered', () => {
    expect(FALLBACK_EDITOR_SETTINGS.lodMiniBelow).toBeLessThan(FALLBACK_EDITOR_SETTINGS.lodCompactBelow)
  })

  it('takes what the server sent and falls back for the rest', () => {
    const cfg = editorSettings({ autosaveMs: 1500, jobPollMs: Number.NaN, toastMs: undefined })
    expect(cfg.autosaveMs).toBe(1500)
    expect(cfg.jobPollMs).toBe(FALLBACK_EDITOR_SETTINGS.jobPollMs)
    expect(cfg.toastMs).toBe(FALLBACK_EDITOR_SETTINGS.toastMs)
    expect(editorSettings(null)).toEqual(FALLBACK_EDITOR_SETTINGS)
    expect(editorSettings(undefined)).toEqual(FALLBACK_EDITOR_SETTINGS)
  })
})
