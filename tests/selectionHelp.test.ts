import { describe, expect, it } from 'vitest'
import { SELECTION_HELP, helpGuides } from '~/utils/selectionHelp'

describe('selectionHelp', () => {
  it('今の方式と、もう一方の方式を返す', () => {
    expect(helpGuides('slideshow').current).toBe(SELECTION_HELP.slideshow)
    expect(helpGuides('slideshow').other).toBe(SELECTION_HELP.tournament)
    expect(helpGuides('tournament').current).toBe(SELECTION_HELP.tournament)
    expect(helpGuides('tournament').other).toBe(SELECTION_HELP.slideshow)
  })
  it('どの節にも項目がある', () => {
    for (const guide of Object.values(SELECTION_HELP)) {
      expect(guide.sections.length).toBeGreaterThan(0)
      for (const section of guide.sections) expect(section.items.length).toBeGreaterThan(0)
    }
  })
})
