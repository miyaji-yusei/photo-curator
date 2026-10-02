// @vitest-environment node
import { describe, expect, it } from 'vitest'
import type { Photo } from '~/types/photo'
import { previewRefreshPlan, withNewThumbnails } from '~/utils/previewRefresh'
import { photos } from './helpers/curatorHarness'

const bare = (list: Photo[], from: number, to: number) =>
  list.map((photo, i) => (i >= from && i < to ? { ...photo, thumbnailPath: null } : photo))

describe('previewRefreshPlan', () => {
  const all = photos('P', 300)
  it('総数が 0・先頭ページに満たないなら全件の読み直し', () => {
    expect(previewRefreshPlan([], 0, 120)).toEqual({ kind: 'full' })
    expect(previewRefreshPlan(all.slice(0, 50), 300, 120)).toEqual({ kind: 'full' })
    expect(previewRefreshPlan(all.slice(0, 50), 50, 120)).toEqual({ kind: 'ids', ids: [] })
  })
  it('サムネイルの無い行の id だけ', () => {
    const list = bare(all, 10, 13)
    expect(previewRefreshPlan(list, 300, 120)).toEqual({ kind: 'ids', ids: ['P-10', 'P-11', 'P-12'] })
  })
})

describe('withNewThumbnails', () => {
  const all = photos('P', 10)
  it('サムネイルが付いたものだけを同じ位置で置き換える', () => {
    const shown = bare(all, 2, 6)
    const fresh = [all[3]!, { ...all[4]!, thumbnailPath: null }]
    const next = withNewThumbnails(shown, fresh)!
    expect(next.map(p => p.id)).toEqual(shown.map(p => p.id))
    expect(next[3]!.thumbnailPath).not.toBeNull()
    expect(next[4]!.thumbnailPath).toBeNull()
    expect(next[0]).toBe(shown[0])
  })
  it('変わるものが無ければ null', () => {
    expect(withNewThumbnails(bare(all, 2, 4), [{ ...all[2]!, thumbnailPath: null }])).toBeNull()
    expect(withNewThumbnails(all, all)).toBeNull()
  })
})

describe('W11 計測: 4,000 枚のうち先頭 2,000 枚を見ている準備中の 3 秒ごとの更新で読む行数', () => {
  it('全件の読み直し → サムネイルが無い行だけ（準備済み 1,000 枚から 30 枚ずつ進む）', () => {
    const total = 4_000
    const loaded = 2_000
    const everything = photos('P', total)
    let prepared = 1_000
    let shown = bare(everything, prepared, total).slice(0, loaded)
    let before = 0
    let after = 0
    for (let step = 0; step < 10; step += 1) {
      before += Math.max(120, shown.length)
      const plan = previewRefreshPlan(shown, total, 120)
      if (plan.kind === 'ids') after += plan.ids.length
      prepared += 30
      if (plan.kind === 'ids') {
        const ready = new Set(plan.ids.filter(id => Number(id.split('-')[1]) < prepared))
        shown = withNewThumbnails(shown, everything.filter(p => ready.has(p.id))) ?? shown
      }
    }
    console.log(`[bench] 10 回の更新で読む行: ${before} 行 → ${after} 行（格子の置き換えは全置換 → 変わった行だけ）`)
    expect(after).toBeLessThan(before)
  })
})
