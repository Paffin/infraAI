import { describe, expect, it } from 'vitest'
import { longestWordEm, wordsOf } from '@/lib/textFit'

describe('the words of a card name', () => {
  it('wrap at spaces and after a hyphen, and nowhere else', () => {
    expect(wordsOf('Kubernetes · app')).toEqual(['Kubernetes', '·', 'app'])
    expect(wordsOf('Multi-region  pilot')).toEqual(['Multi-', 'region', 'pilot'])
    expect(wordsOf('PostgreSQL')).toEqual(['PostgreSQL'])
    expect(wordsOf('')).toEqual([])
  })

  it('the longest of them decides how much room the name needs', () => {
    // without a canvas to measure with (these tests run without a browser) a letter is an average one: the longer word is the wider
    expect(longestWordEm('Балансировщик · стойка 1')).toBeGreaterThan(longestWordEm('Приложение · стойка 1'))
    expect(longestWordEm('Пользователи')).toBe(longestWordEm('Пользователи'))
  })

  it('an empty name still needs some room: the CSS divides by it', () => {
    expect(longestWordEm('')).toBeGreaterThan(0)
  })
})
