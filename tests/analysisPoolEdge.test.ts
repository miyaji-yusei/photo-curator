import { describe, expect, it, vi } from 'vitest'

const analyzePhotoFile = vi.hoisted(() => vi.fn(async (_file: File, _edge?: number) => ({
  thumbnail: null, display: null, dHash: null, capturedAt: null, timestampSource: 'unknown' as const, error: null
})))
vi.mock('~/utils/analyzePhoto', () => ({ analyzePhotoFile }))

import { analyzeAll } from '~/utils/analysisPool'

describe('analyzeAll', () => {
  it('作成時に選んだ長辺を、1 枚ごとの解析に渡す', async () => {
    const file = new File(['x'], 'a.jpg')
    await analyzeAll([{ id: '1', file }, { id: '2', file }], { workers: 1, displayEdge: 1536, onResult: () => {} })
    expect(analyzePhotoFile).toHaveBeenCalledTimes(2)
    for (const call of analyzePhotoFile.mock.calls) expect(call[1]).toBe(1536)
  })
})
