import { parse } from '@vue/compiler-sfc'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

// The attributes of a tab panel (role, id, aria-labelledby, lib/useTabs.ts) are put on the component under the tabs and land on its ROOT element. A component with two
// roots (or text beside the root) cannot take them: Vue drops them with a warning, the panel has no role, and the tab promises a panel that is not there. So every panel
// under a list of tabs has exactly one root; this reads the panels from the files that draw them, so a panel somebody adds to a list is judged too.

const dir = join(process.cwd(), 'src/editor')
const read = (f: string) => readFileSync(join(dir, f), 'utf8')

/** The components a file imports from its own directory by the `./XPanel.vue` / `./SimDetails.vue` pattern. */
function panelsOf(file: string): string[] {
  return [...read(file).matchAll(/from '\.\/([A-Za-z]+\.vue)'/g)].map((m) => m[1])
}

/** Root nodes of a template, a `v-if` / `v-else-if` / `v-else` chain being one. */
function roots(file: string): string[] {
  const ast = parse(read(file)).descriptor.template?.ast
  const out: string[] = []
  for (const c of ast?.children ?? []) {
    if (c.type === 3) continue // a comment
    if (c.type === 2 && !c.content.trim()) continue // white space
    if (c.type === 1 && c.props.some((p) => p.type === 7 && (p.name === 'else' || p.name === 'else-if'))) continue
    out.push(c.type === 1 ? c.tag : c.type === 2 ? 'text' : 'interpolation')
  }
  return out
}

describe('the panels under a list of tabs take the attributes of a tab panel', () => {
  const files = [...new Set([...panelsOf('RightPanel.vue'), ...panelsOf('AnalysisPanel.vue')])]

  it('are found in the files that draw them', () => {
    // the failure's own panel, the checks, the requirements, the analysis and its four views
    for (const f of ['SimDetails.vue', 'ChecksPanel.vue', 'RequirementsPanel.vue', 'AnalysisPanel.vue', 'CapacityPanel.vue', 'AvailabilityPanel.vue', 'CostPanel.vue', 'SitesPanel.vue']) expect(files, f).toContain(f)
  })

  for (const f of ['SimDetails.vue', 'ChecksPanel.vue', 'RequirementsPanel.vue', 'ChatPanel.vue', 'AnalysisPanel.vue', 'CapacityPanel.vue', 'AvailabilityPanel.vue', 'CostPanel.vue', 'SitesPanel.vue']) {
    it(`${f} has one root element`, () => {
      expect(roots(f)).toHaveLength(1)
      expect(roots(f)[0]).not.toMatch(/text|interpolation/)
    })
  }

  it('every panel the files import is judged', () => {
    for (const f of files) expect(roots(f), f).toHaveLength(1)
  })
})
