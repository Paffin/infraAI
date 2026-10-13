import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import type { Architecture, Catalog, Report } from '../src/api/types'
import { indexCatalog } from '../src/lib/catalog'

/**
 * Real engine output: `{ architecture, report }` recorded with `cargo run -p engine --example report -- <input> en --json`
 * (see tests/fixtures/README.md). The suites run on what the backend actually returns, not on hand-written look-alikes.
 */
export function fixture(name: string): { architecture: Architecture; report: Report } {
  return JSON.parse(readFileSync(join(process.cwd(), 'tests', 'fixtures', `${name}.json`), 'utf8'))
}

/** The shipped catalog (the same file the backend seeds its database from). */
export function realCatalog(): Catalog {
  const raw = JSON.parse(readFileSync(join(process.cwd(), '..', 'backend', 'crates', 'engine', 'defaults', 'catalog.json'), 'utf8'))
  return { categories: raw.categories, zoneTypes: raw.zoneTypes, edgeTypes: raw.edgeTypes, commonProps: raw.commonProps, kinds: raw.kinds }
}

export const realIndex = () => indexCatalog(realCatalog())

/** Rule id → check type, from the shipped rule catalog (what the UI gets as `app.rules`). */
export function ruleCheck(id: string): string | undefined {
  const rules: { id: string; check: string }[] = JSON.parse(readFileSync(join(process.cwd(), '..', 'backend', 'crates', 'engine', 'defaults', 'rules.json'), 'utf8'))
  return rules.find((r) => r.id === id)?.check
}
