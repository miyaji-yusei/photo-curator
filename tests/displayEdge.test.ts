import { describe, expect, it } from 'vitest'
import { builtDisplayCount, canChangeDisplayEdge, displayEdgeLabel, displayEdgePlan, estimateDisplayMegabytes, nearestDisplayEdge } from '~/utils/displayEdge'

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

describe('displayEdgePlan', () => {
  const base = { current: 1024, next: 1536, builtCount: 0 }

  it('まだ何も作られていなければ、警告なしで保存だけ', () => {
    expect(displayEdgePlan({ ...base, builtCount: 0 })).toBe('save')
  })
  it('作られた画像があり、値が変わるなら、作り直しの確認', () => {
    expect(displayEdgePlan({ ...base, builtCount: 10 })).toBe('confirm')
    expect(displayEdgePlan({ ...base, next: 768, builtCount: 1 })).toBe('confirm')
  })
  it('同じ値なら何もしない', () => {
    expect(displayEdgePlan({ ...base, next: 1024, builtCount: 10 })).toBe('same')
  })
  it('作り直せない（原本が無い）なら変えられない', () => {
    expect(displayEdgePlan({ ...base, canRebuild: false, builtCount: 10 })).toBe('locked')
    expect(canChangeDisplayEdge(false)).toBe(false)
    expect(canChangeDisplayEdge(true)).toBe(true)
    expect(canChangeDisplayEdge(undefined)).toBe(true)
  })
  it('作り直しが要らない環境（Web の Amazon）は、作られた枚数があっても保存だけ', () => {
    expect(displayEdgePlan({ ...base, rebuildsOnChange: false, builtCount: 10 })).toBe('save')
  })
  it('作られた枚数は、写真の数から残りを引く', () => {
    expect(builtDisplayCount(100, 100)).toBe(0)
    expect(builtDisplayCount(100, 30)).toBe(70)
    expect(builtDisplayCount(100, 150)).toBe(0)
  })
})
