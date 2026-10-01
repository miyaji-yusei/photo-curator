// @vitest-environment node
import 'fake-indexeddb/auto'
import { describe, expect, it, vi } from 'vitest'

const ran = vi.hoisted(() => ({ ids: [] as string[][] }))
vi.mock('~/utils/analysisPool', async importOriginal => ({
  ...(await importOriginal<typeof import('~/utils/analysisPool')>()),
  analyzeAll: vi.fn(async (jobs: { id: string }[]) => { ran.ids.push(jobs.map(job => job.id)) })
}))

import { orderForAnalysis } from '~/utils/analysisOrder'
import { createIdbStore } from '~/composables/backends/web/store'
import { createIdbBlobStore } from '~/composables/backends/web/blobStore'
import { createLocalBackend } from '~/composables/backends/local'
import type { SourceIOSet } from '~/composables/backends/web/sourceIO'
import type { StoredPhoto, StoredProject } from '~/utils/browserStore'

const item = (id: string, capturedAt: number | null, relativePath = id) => ({ id, capturedAt, relativePath })
const order = (items: ReturnType<typeof item>[]) => orderForAnalysis(items, x => x).map(x => x.id)

describe('orderForAnalysis', () => {
  it('撮影時刻の昇順、同時刻は相対パス', () => {
    expect(order([item('c', 30), item('b2', 10, 'b/2'), item('a', 20), item('b1', 10, 'b/1')]))
      .toEqual(['b1', 'b2', 'a', 'c'])
  })
  it('撮影時刻の無いものは最後に、渡された順のまま', () => {
    expect(order([item('z', null), item('a', 5), item('y', null), item('b', 1)]))
      .toEqual(['b', 'a', 'z', 'y'])
  })
  it('全部分からないなら渡された順のまま・元の配列は変えない', () => {
    const input = [item('b', null), item('a', null)]
    expect(order(input)).toEqual(['b', 'a'])
    expect(input.map(x => x.id)).toEqual(['b', 'a'])
  })
})

describe('Web の準備は撮影時刻の昇順で解析する', () => {
  it('startBackgroundAnalysis が昇順の job を渡す', async () => {
    const store = createIdbStore()
    const project: StoredProject = {
      id: 'p', name: 'x', source: { kind: 'dev', root: '/r' }, photoCount: 4, status: 'ready',
      createdAt: 1, updatedAt: 1, burstThreshold: null, burstThresholdLearnedAt: null
    }
    const mk = (id: string, capturedAt: number | null): StoredPhoto => ({
      id, projectId: 'p', path: `${id}.jpg`, relativePath: `${id}.jpg`, name: `${id}.jpg`,
      capturedAt, timestampSource: 'unknown', dHash: null, rating: 0, isMissing: false, analysisError: null
    })
    await store.putProject(project)
    await store.putPhotos([mk('a-n1', null), mk('b-late', 300), mk('c-early', 100), mk('d-n2', null)])
    const io = { list: async () => [], readFile: async () => new File([], 'x') } as never
    const sourceIO = { picker: {}, forHandle: () => io, forDev: () => io } as unknown as SourceIOSet
    const backend = createLocalBackend({ store, blobStore: createIdbBlobStore(), sourceIO })
    await backend.startBackgroundAnalysis('p')
    expect(ran.ids).toEqual([['c-early', 'b-late', 'a-n1', 'd-n2']])
  })
})
