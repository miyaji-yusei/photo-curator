import { describe, expect, it } from 'vitest'
import type { Photo } from '~/types/photo'
import { buildCoreInputs, maxNeighborDistance, sortForCore, toPhotoRef } from '~/utils/coreInputs'

const photo = (name: string, capturedAt: number | null, id = name): Photo => ({
  id, projectId: 'p', path: `/x/${name}`, relativePath: name, name, capturedAt,
  dHash: '00ff00ff00ff00ff', rating: 0, thumbnailPath: null, displayPath: null
})

describe('sortForCore', () => {
  it('撮影順。撮影時刻が無いものは最後、同じならファイル名順', () => {
    const sorted = sortForCore([
      photo('c.jpg', null), photo('b.jpg', 200), photo('z.jpg', 100), photo('a.jpg', 200), photo('a0.jpg', null)
    ])
    expect(sorted.map(item => item.name)).toEqual(['z.jpg', 'a.jpg', 'b.jpg', 'a0.jpg', 'c.jpg'])
  })
})

describe('toPhotoRef / buildCoreInputs', () => {
  it('鍵は relativePath、指紋の版は 2', () => {
    expect(toPhotoRef(photo('a.jpg', 5))).toEqual({
      relative_path: 'a.jpg', captured_at: 5, d_hash: '00ff00ff00ff00ff', d_hash_version: 2
    })
  })

  it('relativePath と id の両方から引ける', () => {
    const inputs = buildCoreInputs([photo('b.jpg', 2, 'id-b'), photo('a.jpg', 1, 'id-a')])
    expect(inputs.refs.map(ref => ref.relative_path)).toEqual(['a.jpg', 'b.jpg'])
    expect(inputs.byPath.get('b.jpg')?.id).toBe('id-b')
    expect(inputs.byId.get('id-a')?.relativePath).toBe('a.jpg')
  })
})

describe('maxNeighborDistance', () => {
  const distance = (left: string, right: string) => Math.abs(parseInt(left, 16) - parseInt(right, 16))
  const ref = (at: number | null, hash: string | null) =>
    ({ relative_path: 'x', captured_at: at, d_hash: hash, d_hash_version: 2 })

  it('時間が近い隣どうしの中で一番遠い距離', () => {
    const refs = [ref(0, '0'), ref(1000, '3'), ref(2000, 'a'), ref(60_000, 'ff')]
    expect(maxNeighborDistance(refs, 4000, distance)).toBe(7)
  })

  it('候補が無ければ既定値', () => {
    expect(maxNeighborDistance([ref(0, '0'), ref(60_000, '1')], 4000, distance)).toBe(64)
    expect(maxNeighborDistance([ref(0, null), ref(1, '1')], 4000, distance, 30)).toBe(30)
  })
})
