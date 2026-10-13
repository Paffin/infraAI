// The guided tour: what it shows (data), which steps can be shown on this screen, and where the card goes.
// The texts are translations (`tour.<tour>.<step>.title` / `.body`); the selectors point at `data-tour` hooks in the templates.

export type TourId = 'projects' | 'editor'
export type Placement = 'top' | 'bottom' | 'left' | 'right'

export interface TourStep {
  id: string
  /** CSS selector of the element to highlight; no target: a card in the middle of the screen (an opening or a closing word) */
  target?: string
  placement?: Placement
  /** only for administrators (the step points at the administration entry) */
  admin?: boolean
}

export interface TourDef {
  id: TourId
  /** the route the tour belongs to (`router` route name) */
  route: string
  steps: TourStep[]
}

export const TOURS: TourDef[] = [
  {
    id: 'projects',
    route: 'projects',
    steps: [
      { id: 'new', target: '[data-tour="new-project"]', placement: 'bottom' },
      { id: 'list', target: '[data-tour="project-list"]', placement: 'top' },
      { id: 'admin', target: '[data-tour="admin"]', placement: 'bottom', admin: true },
      { id: 'help', target: '[data-tour="help"]', placement: 'bottom' },
    ],
  },
  {
    id: 'editor',
    route: 'editor',
    steps: [
      { id: 'intro' },
      { id: 'library', target: '[data-tour="library"]', placement: 'right' },
      { id: 'prompt', target: '[data-tour="prompt"]', placement: 'bottom' },
      { id: 'score', target: '[data-tour="score"]', placement: 'bottom' },
      { id: 'checks', target: '[data-tour="checks"]', placement: 'left' },
      { id: 'whatif', target: '[data-tour="whatif"]', placement: 'top' },
      { id: 'export', target: '[data-tour="export"]', placement: 'bottom' },
      { id: 'palette', target: '[data-tour="palette"]', placement: 'bottom' },
      { id: 'done' },
    ],
  },
]

export const tourFor = (route: unknown): TourDef | undefined => TOURS.find((t) => t.route === route)
export const tourById = (id: string): TourDef | undefined => TOURS.find((t) => t.id === id)

/** Translation keys of a step. */
export const stepKeys = (tour: TourId, step: string) => ({ title: `tour.${tour}.${step}.title`, body: `tour.${tour}.${step}.body` })

/** The steps that can be shown right now: a step whose element is not on the screen is left out (a collapsed panel, a role without the entry). */
export function visibleSteps(def: TourDef, present: (selector: string) => boolean, isAdmin: boolean): TourStep[] {
  return def.steps.filter((s) => (!s.admin || isAdmin) && (!s.target || present(s.target)))
}

/** Where the "seen" mark of a tour lives for a user. */
export const seenKey = (userId: string | null | undefined, tour: TourId) => `ia_tour_${userId ?? 'anon'}_${tour}`

// ------------------------------------------------------------------------------------------ geometry

export interface Rect { x: number; y: number; w: number; h: number }
export interface Size { w: number; h: number }

/** The highlighted area: the element's box with some air, kept inside the viewport. */
export function holeOf(r: Rect, pad: number, viewport: Size): Rect {
  const x = Math.max(0, r.x - pad)
  const y = Math.max(0, r.y - pad)
  const right = Math.min(viewport.w, r.x + r.w + pad)
  const bottom = Math.min(viewport.h, r.y + r.h + pad)
  return { x, y, w: Math.max(0, right - x), h: Math.max(0, bottom - y) }
}

const OPPOSITE: Record<Placement, Placement> = { top: 'bottom', bottom: 'top', left: 'right', right: 'left' }

/**
 * Where the card goes next to the highlighted area. The preferred side first, then the opposite one, then the other two;
 * the first that fits inside the viewport wins; when none does the card sits in the middle. Along the free axis the card
 * is centred on the area and nudged back into the viewport.
 */
export function placeCard(hole: Rect | null, card: Size, viewport: Size, prefer: Placement = 'bottom', gap = 14, margin = 12): { x: number; y: number; placement: Placement | 'center' } {
  const clampX = (x: number) => Math.min(Math.max(margin, x), Math.max(margin, viewport.w - card.w - margin))
  const clampY = (y: number) => Math.min(Math.max(margin, y), Math.max(margin, viewport.h - card.h - margin))
  const middle = { x: clampX((viewport.w - card.w) / 2), y: clampY((viewport.h - card.h) / 2), placement: 'center' as const }
  if (!hole) return middle
  const order: Placement[] = [prefer, OPPOSITE[prefer], ...(prefer === 'top' || prefer === 'bottom' ? (['right', 'left'] as const) : (['bottom', 'top'] as const))]
  for (const p of order) {
    let x: number
    let y: number
    if (p === 'bottom') { x = clampX(hole.x + hole.w / 2 - card.w / 2); y = hole.y + hole.h + gap }
    else if (p === 'top') { x = clampX(hole.x + hole.w / 2 - card.w / 2); y = hole.y - gap - card.h }
    else if (p === 'right') { x = hole.x + hole.w + gap; y = clampY(hole.y + hole.h / 2 - card.h / 2) }
    else { x = hole.x - gap - card.w; y = clampY(hole.y + hole.h / 2 - card.h / 2) }
    if (x >= margin && y >= margin && x + card.w <= viewport.w - margin && y + card.h <= viewport.h - margin) return { x, y, placement: p }
  }
  return middle
}
