import { describe, expect, it, vi } from 'vitest'
import { fetchPool } from '~/utils/amazonPool'
import { directFetcher, relayFetcher } from '~/composables/backends/web/fetcher'

const blob = () => new Blob(['x'])

describe('fetchPool', () => {
  it('ぜんぶ取れれば最後まで回る', async () => {
    const seen: number[] = []
    const report = await fetchPool({
      items: Array.from({ length: 10 }, (_, i) => i),
      concurrency: 4,
      giveUpAfter: 4,
      fetch: async () => blob(),
      onResult: item => { seen.push(item) }
    })
    expect(report).toEqual({ attempted: 10, succeeded: 10, gaveUp: false })
    expect(seen.sort((a, b) => a - b)).toEqual([0, 1, 2, 3, 4, 5, 6, 7, 8, 9])
  })

  it('最初の 4 枚が全部失敗したら打ち切る（中継が無いとみなす）', async () => {
    const fetch = vi.fn(async () => null)
    const report = await fetchPool({
      items: Array.from({ length: 4000 }, (_, i) => i),
      concurrency: 4,
      giveUpAfter: 4,
      fetch,
      onResult: () => {}
    })
    expect(report.gaveUp).toBe(true)
    expect(report.succeeded).toBe(0)
    // 最初の 4 枚だけで止まる（4,000 回は叩かない）。
    expect(fetch).toHaveBeenCalledTimes(4)
    expect(report.attempted).toBe(4)
  })

  it('1 枚でも取れたら打ち切らない（読めない写真は飛ばして続ける）', async () => {
    const report = await fetchPool({
      items: Array.from({ length: 20 }, (_, i) => i),
      concurrency: 4,
      giveUpAfter: 4,
      fetch: async item => (item === 3 ? blob() : null),
      onResult: () => {}
    })
    expect(report.gaveUp).toBe(false)
    expect(report.attempted).toBe(20)
    expect(report.succeeded).toBe(1)
  })

  it('fetch が例外を投げても失敗として数える', async () => {
    const report = await fetchPool({
      items: [1, 2, 3, 4, 5, 6],
      concurrency: 2,
      giveUpAfter: 4,
      fetch: async () => { throw new Error('boom') },
      onResult: () => {}
    })
    expect(report.gaveUp).toBe(true)
  })

  it('4 枚に満たない件数でも、ぜんぶ失敗なら打ち切りとして返す', async () => {
    const report = await fetchPool({
      items: [1, 2], concurrency: 4, giveUpAfter: 4, fetch: async () => null, onResult: () => {}
    })
    expect(report).toEqual({ attempted: 2, succeeded: 0, gaveUp: true })
  })

  it('giveUpAfter が無ければ、失敗が続いても最後まで回す', async () => {
    const fetch = vi.fn(async () => null)
    const report = await fetchPool({ items: [1, 2, 3, 4, 5, 6], concurrency: 2, fetch, onResult: () => {} })
    expect(report).toEqual({ attempted: 6, succeeded: 0, gaveUp: false })
  })

  it('中断されたら新しい取得を始めない', async () => {
    let stop = false
    const fetch = vi.fn(async () => { stop = true; return blob() })
    await fetchPool({
      items: Array.from({ length: 50 }, (_, i) => i),
      concurrency: 1,
      isCancelled: () => stop,
      fetch,
      onResult: () => {}
    })
    expect(fetch).toHaveBeenCalledTimes(1)
  })
})

describe('fetcher', () => {
  it('relayFetcher は /api/amazon/image を通し、失敗は null', async () => {
    const calls: string[] = []
    vi.stubGlobal('fetch', async (url: string) => {
      calls.push(url)
      return new Response('nope', { status: 404 })
    })
    expect(await relayFetcher.fetchBytes('https://content-x.drive.amazonaws.com/a', 160)).toBeNull()
    expect(calls[0]).toContain('/api/amazon/image?tempLink=')
    expect(calls[0]).toContain('edge=160')
    vi.unstubAllGlobals()
  })

  it('directFetcher は tempLink をそのまま取る（縮小は viewBox）', async () => {
    const calls: string[] = []
    vi.stubGlobal('fetch', async (url: string) => {
      calls.push(url)
      return new Response('ok')
    })
    expect(await directFetcher.fetchBytes('https://cdn/x')).not.toBeNull()
    expect(await directFetcher.fetchBytes('https://cdn/x', 160)).not.toBeNull()
    expect(calls).toEqual(['https://cdn/x', 'https://cdn/x?viewBox=160,160'])
    vi.unstubAllGlobals()
  })

  it('directFetcher は例外も null にする', async () => {
    vi.stubGlobal('fetch', async () => { throw new Error('cors') })
    expect(await directFetcher.fetchBytes('https://cdn/x')).toBeNull()
    vi.unstubAllGlobals()
  })
})
