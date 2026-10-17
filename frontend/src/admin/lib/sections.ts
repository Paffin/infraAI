// The pages of the admin panel by name. Kept apart from `configKinds.ts` on purpose: the title of the tab (`lib/pageTitle.ts`, loaded with the first page) needs only these few
// lines, and `configKinds` pulls in the JSON scanner and all the editors' tables, which belong to the admin chunk and not to the sign-in page.

/** Sections with several pages and the page that opens first. */
export const SECTION_SUBS: Record<string, string> = { catalog: 'kind', placement: 'pattern' }

export const SECTION_IDS = ['settings', 'llm', 'catalog', 'placement', 'rules', 'templates', 'iac', 'prompts', 'engine', 'backup', 'users', 'audit', 'system'] as const
export type SectionId = typeof SECTION_IDS[number]

export function normalizeSection(s: unknown): SectionId {
  return (SECTION_IDS as readonly string[]).includes(String(s)) ? (s as SectionId) : 'settings'
}
