import { describe, expect, it } from 'vitest'
import { displayEdgeLabel, estimateDisplayMegabytes, nearestDisplayEdge } from '~/utils/displayEdge'

describe('displayEdge', () => {
  it('1024px は 2,000 枚あたり約 200MB を基準にし、長辺の 2 乗に比例する', () => {
    expect(estimateDisplayMegabytes(1024)).toBe(200)
    expect(estimateDisplayMegabytes(1536)).toBe(450)
    expect(estimateDisplayMegabytes(768)).toBe(110)
    expect(estimateDisplayMegabytes(1920)).toBe(700)
  })

  it('選択肢の文に長辺と容量が入る', () => {
    expect(displayEdgeLabel(1024)).toBe('1024px（2,000 枚あたり約 200MB）')
  })

  it('選択肢に無い値は近いものに寄せる', () => {
    expect(nearestDisplayEdge(1100)).toBe(1024)
    expect(nearestDisplayEdge(5000)).toBe(1920)
  })
})
