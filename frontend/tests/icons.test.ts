import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { ICONS, iconFor } from '@/lib/icons'
import { Box, Database } from 'lucide-vue-next'

const catalog = JSON.parse(readFileSync(join(process.cwd(), '..', 'backend', 'crates', 'engine', 'defaults', 'catalog.json'), 'utf8')) as {
  categories: { id: string; icon?: string }[]
  zoneTypes: { id: string; icon?: string }[]
  kinds: { id: string; icon?: string; category: string }[]
}

describe('the icons of the shipped catalog', () => {
  it('every kind, zone type and category names an icon the UI knows', () => {
    const missing: string[] = []
    for (const [owner, items] of [['kind', catalog.kinds], ['zone type', catalog.zoneTypes], ['category', catalog.categories]] as const) {
      for (const x of items) if (!x.icon || !ICONS[x.icon]) missing.push(`${owner} ${x.id}: ${x.icon ?? 'no icon'}`)
    }
    expect(missing).toEqual([])
  })

  it('the cards of a design do not all wear the same face', () => {
    expect(new Set(catalog.kinds.map((k) => k.icon)).size).toBeGreaterThan(30)
  })
})

describe('iconFor', () => {
  it('takes the first name the registry knows', () => {
    expect(iconFor('database')).toBe(Database)
    expect(iconFor('no-such-icon', 'database')).toBe(Database)
    expect(iconFor(undefined, null, 'database')).toBe(Database)
  })

  it('draws a plain box rather than a hole when no name is known', () => {
    expect(iconFor()).toBe(Box)
    expect(iconFor('typo', undefined)).toBe(Box)
  })
})
