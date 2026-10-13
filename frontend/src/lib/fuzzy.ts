/** Case-, `ё`- and diacritics-insensitive form used for matching. */
export function fold(s: string): string {
  return s.toLowerCase().replace(/ё/g, 'е').normalize('NFKD').replace(/[̀-ͯ]/g, '')
}

const WORD_START = /[\s\-_/.·:()[\]]/

/** Score of one token in one text: lower is better, `null` = no match. */
function tokenScore(token: string, text: string): number | null {
  const at = text.indexOf(token)
  if (at >= 0) return (at === 0 || WORD_START.test(text[at - 1]) ? 0 : 20) + at * 0.1
  // typos and abbreviations ("pgsql" → "PostgreSQL"): the letters appear in order
  if (token.length < 3) return null
  let pos = -1
  let gaps = 0
  for (const ch of token) {
    const next = text.indexOf(ch, pos + 1)
    if (next < 0) return null
    gaps += next - pos - 1
    pos = next
  }
  return 100 + gaps
}

/**
 * How well `query` matches any of `texts` (the first text counts most): every whitespace-separated token has to
 * match, as a substring (best at a word start) or – from three letters on – as an ordered subsequence.
 * Lower is better; `null` = no match. An empty query matches everything equally.
 */
export function fuzzyScore(query: string, texts: string[]): number | null {
  const tokens = fold(query).split(/\s+/).filter(Boolean)
  if (!tokens.length) return 0
  const folded = texts.map(fold)
  let total = 0
  for (const token of tokens) {
    let best: number | null = null
    folded.forEach((text, i) => {
      const s = tokenScore(token, text)
      if (s !== null) {
        const weighted = s + i * 8
        if (best === null || weighted < best) best = weighted
      }
    })
    if (best === null) return null
    total += best
  }
  return total
}

/** Items that match, best first (stable for equal scores). */
export function rank<T>(items: T[], query: string, texts: (x: T) => string[]): T[] {
  const scored: { x: T; s: number; i: number }[] = []
  items.forEach((x, i) => {
    const s = fuzzyScore(query, texts(x))
    if (s !== null) scored.push({ x, s, i })
  })
  return scored.sort((a, b) => a.s - b.s || a.i - b.i).map((e) => e.x)
}
