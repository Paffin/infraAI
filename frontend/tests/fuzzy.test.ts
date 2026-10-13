import { describe, expect, it } from 'vitest'
import { fold, fuzzyScore, rank } from '../src/lib/fuzzy'

describe('fuzzy search of the command palette', () => {
  it('ignores case, ё and diacritics', () => {
    expect(fold('Ёмкость')).toBe('емкость')
    expect(fuzzyScore('емкость', ['Ёмкость'])).not.toBeNull()
    expect(fuzzyScore('POSTGRES', ['PostgreSQL primary'])).not.toBeNull()
    expect(fuzzyScore('cafe', ['Café'])).not.toBeNull()
  })

  it('needs every token to match', () => {
    expect(fuzzyScore('postgres primary', ['PostgreSQL primary', 'ЦОД-1'])).not.toBeNull()
    expect(fuzzyScore('postgres replica', ['PostgreSQL primary'])).toBeNull()
    expect(fuzzyScore('', ['anything'])).toBe(0)
  })

  it('prefers word starts to the middle of a word, and the first text to the later ones', () => {
    expect(fuzzyScore('app', ['App DC1'])!).toBeLessThan(fuzzyScore('app', ['Wrapper'])!)
    expect(fuzzyScore('dc1', ['DC1', 'other'])!).toBeLessThan(fuzzyScore('dc1', ['other', 'DC1'])!)
  })

  it('finds abbreviations as ordered letters but not from fewer than three', () => {
    expect(fuzzyScore('pgsql', ['PostgreSQL'])).not.toBeNull()
    expect(fuzzyScore('post', ['PostgreSQL'])).not.toBeNull() // a plain prefix
    expect(fuzzyScore('pg', ['PostgreSQL'])).toBeNull() // two letters must be a real substring
    expect(fuzzyScore('lsp', ['PostgreSQL'])).toBeNull() // order matters
  })

  it('ranks the best matches first and keeps the original order of equal ones', () => {
    const items = ['Redis replica', 'Kafka', 'Redis master', 'Registry']
    expect(rank(items, 'redis', (x) => [x])).toEqual(['Redis replica', 'Redis master'])
    expect(rank(items, 'ka', (x) => [x])[0]).toBe('Kafka')
    expect(rank(items, '', (x) => [x])).toEqual(items)
  })
})
