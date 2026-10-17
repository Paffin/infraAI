import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'
import { describe, expect, it } from 'vitest'

// "No hard-coded numbers": a pause of the interface is a setting (`editor.*`, delivered by /bootstrap) or a token of the motion system (`lib/motion.ts`), not a number
// in a `setTimeout`. This is a ratchet over the source of the application: every `setTimeout` whose delay holds a literal of 100 ms or more is counted per file, the
// files below are the ones that had them when the ratchet was set, and no file may have more than it has here. The save path (the editor store and the two modules
// it is made of) has none at all, and a new file starts with none.

const SRC = join(process.cwd(), 'src')

/** The files that still have such a delay, and how many. Lower a number when you remove one; never raise one: take the pause from a setting or a token. */
const ALLOWED: Record<string, number> = {
  'admin/components/CopyButton.vue': 1, // "copied" stays for a moment
  'admin/components/ModelPicker.vue': 3, // search debounce, closing the list after a blur, the lookup of a model's details
  'admin/lib/useSettingsEditor.ts': 1, // "saved" fades
  'admin/sections/AuditSection.vue': 1, // search debounce
  'admin/sections/BackupSection.vue': 1, // revoking the object URL of a download
  'components/TourHost.vue': 1, // polling for the target of a tour step
  'editor/Canvas.vue': 1, // the diagram settles before it is fitted again
  'editor/ExportModal.vue': 1, // "copied" stays for a moment
  'editor/Inspector.vue': 1, // the flash of a field that was asked for
  'editor/PlacementWizard.vue': 2, // debounces of the preview and the comparison
  'editor/RequirementsPanel.vue': 1, // the flash of a field that was asked for
}

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const p = join(dir, name)
    return statSync(p).isDirectory() ? files(p) : /\.(ts|vue)$/.test(name) ? [p] : []
  })
}

/** The text of the arguments of the call that opens at `from` (just after the parenthesis), up to its closing parenthesis; strings are skipped over. */
function args(text: string, from: number): string {
  let depth = 1
  let i = from
  for (; i < text.length && depth > 0; i++) {
    const c = text[i]
    if (c === '"' || c === "'" || c === '`') {
      for (i++; i < text.length && text[i] !== c; i++) if (text[i] === '\\') i++
    } else if (c === '(') depth++
    else if (c === ')') depth--
  }
  return text.slice(from, i - 1)
}

/** The top-level arguments of a call, as text. */
function split(a: string): string[] {
  const out: string[] = []
  let depth = 0
  let cur = ''
  for (let i = 0; i < a.length; i++) {
    const c = a[i]
    if (c === '"' || c === "'" || c === '`') {
      let j = i + 1
      for (; j < a.length && a[j] !== c; j++) if (a[j] === '\\') j++
      cur += a.slice(i, j + 1)
      i = j
      continue
    }
    if ('([{'.includes(c)) depth++
    if (')]}'.includes(c)) depth--
    if (c === ',' && depth === 0) { out.push(cur); cur = ''; continue }
    cur += c
  }
  out.push(cur)
  return out
}

/** How many `setTimeout` calls of the text have a number of 100 or more in their delay (`1200`, `highlightMs + 100`, `5_000`). */
export function literalDelays(text: string): number {
  let n = 0
  for (const m of text.matchAll(/\bsetTimeout\(/g)) {
    const delay = split(args(text, m.index! + m[0].length))[1]
    if (delay && [...delay.matchAll(/(?<![\w.$])(\d[\d_]*)(?![\w$])/g)].some((d) => Number(d[1].replace(/_/g, '')) >= 100)) n++
  }
  return n
}

const found = new Map<string, number>()
for (const f of files(SRC)) {
  const n = literalDelays(readFileSync(f, 'utf8'))
  if (n > 0) found.set(relative(SRC, f).split('\\').join('/'), n)
}

describe('the scanner', () => {
  it('counts a literal delay of 100 ms or more, in every shape it takes', () => {
    expect(literalDelays('setTimeout(() => go(), 1200)')).toBe(1)
    expect(literalDelays('setTimeout(() => { a(); b(1, 2) }, 150)')).toBe(1)
    expect(literalDelays('setTimeout(r, 100)')).toBe(1)
    expect(literalDelays('setTimeout(f, app.cfg.highlightMs + 100)')).toBe(1)
    expect(literalDelays('setTimeout(f, 5_000)')).toBe(1)
    expect(literalDelays("new Promise((r) => setTimeout(r, 150)); setTimeout(() => x(), 2300)")).toBe(2)
  })

  it('does not count what is not a literal delay', () => {
    expect(literalDelays('setTimeout(f, app.editorCfg.autosaveMs)')).toBe(0)
    expect(literalDelays('setTimeout(f, DURATIONS.base)')).toBe(0)
    expect(literalDelays('setTimeout(f, 0)')).toBe(0)
    expect(literalDelays('setTimeout(f, 99)')).toBe(0)
    expect(literalDelays('setTimeout(f, TIMER_SLACK_MS)')).toBe(0)
    expect(literalDelays("setTimeout(() => say('wait 500 ms, then 1000'), delay)")).toBe(0)
    expect(literalDelays('setTimeout(f, x * 1.5)')).toBe(0)
    expect(literalDelays('clearTimeout(timer)')).toBe(0)
  })
})

describe('pauses of the interface are settings or tokens, not numbers in the code', () => {
  it('no file has more literal delays than it had when the ratchet was set, and no new file has any', () => {
    const over = [...found].filter(([f, n]) => n > (ALLOWED[f] ?? 0)).map(([f, n]) => `${f}: ${n} (allowed ${ALLOWED[f] ?? 0})`)
    expect(over, 'take the pause from a setting (backend/crates/server/defaults/settings.json, group editor) or from DURATIONS (lib/motion.ts)').toEqual([])
  })

  it('the allowance names only files that exist and still have one (lower it when you remove a literal)', () => {
    const stale = Object.entries(ALLOWED).filter(([f, n]) => (found.get(f) ?? 0) < n).map(([f, n]) => `${f}: allowed ${n}, has ${found.get(f) ?? 0}`)
    expect(stale).toEqual([])
  })

  it('the save path has none: the editor store, the history and the save queue take every pause from the settings', () => {
    for (const f of ['stores/editor.ts', 'stores/unsaved.ts', 'lib/history.ts', 'lib/saveQueue.ts', 'lib/unsavedBackup.ts', 'lib/session.ts', 'editor/SaveStatusBar.vue']) {
      expect(found.get(f) ?? 0, f).toBe(0)
    }
  })
})
