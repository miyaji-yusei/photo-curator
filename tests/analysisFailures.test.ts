// @vitest-environment node
import 'fake-indexeddb/auto'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { AnalysisJob } from '~/utils/analysisPool'
import type { AnalyzedPhoto } from '~/utils/analyzePhoto'

// 解析の本体は動かさず、渡された仕事（job）を記録し、決めた結果を onResult に返す。
const run = vi.hoisted(() => ({
  jobs: [] as { id: string, hadFile: boolean }[],
  results: {} as Record<string, unknown>
}))
vi.mock('~/utils/analysisPool', async importOriginal => ({
  ...(await importOriginal<typeof import('~/utils/analysisPool')>()),
  analyzeAll: vi.fn(async (
    jobs: AnalysisJob[],
    options: { onResult: (id: string, analyzed: unknown, file: File | null) => Promise<void> | void }
  ) => {
    for (const job of jobs) {
      run.jobs.push({ id: job.id, hadFile: !!job.file })
      const file = job.file ?? await job.load?.() ?? null
      await options.onResult(job.id, run.results[job.id], file)
    }
  })
}))

import { createIdbStore } from '~/composables/backends/web/store'
import { createIdbBlobStore } from '~/composables/backends/web/blobStore'
import { createLocalBackend } from '~/composables/backends/local'
import type { SourceIOSet } from '~/composables/backends/web/sourceIO'
import type { StoredPhoto, StoredProject } from '~/utils/browserStore'
import { isUndecodableError, analyzePhotoFile } from '~/utils/analyzePhoto'
import { isSettledUnsupported, needsAnalysis, selectableCount } from '~/utils/analysisFailures'
import { buildCoreInputs } from '~/utils/coreInputs'

const row = (id: string, extra: Partial<StoredPhoto> = {}): StoredPhoto => ({
  id, projectId: 'p', path: `${id}.jpg`, relativePath: `${id}.jpg`, name: `${id}.jpg`,
  capturedAt: 100, timestampSource: 'unknown', dHash: null, rating: 0, isMissing: false, analysisError: null, ...extra
})
const project: StoredProject = {
  id: 'p', name: 'x', source: { kind: 'dev', root: '/r' }, photoCount: 6, status: 'ready',
  createdAt: 1, updatedAt: 1, burstThreshold: null, burstThresholdLearnedAt: null
}
const unsupportedRow = (id: string) => row(id, {
  analysisError: '画像を読み込めませんでした。', analysisErrorKind: 'unsupported',
  analysisFailedSize: 10, analysisFailedModified: 1000
})

async function setup(files: Record<string, { size: number, modified: number }>, rows: StoredPhoto[]) {
  const store = createIdbStore()
  // 前のテストの行を残さない。
  await store.deleteProject('p', (await store.photosOfProject('p')).map(item => item.id))
  await store.putProject(project)
  await store.putPhotos(rows)
  const io = {
    list: async () => [],
    readFile: async (_sub: string, name: string) => {
      const spec = files[name]
      if (!spec) throw new Error('読めません')
      return new File(['x'.repeat(spec.size)], name, { lastModified: spec.modified })
    }
  } as never
  const sourceIO = { picker: {}, forHandle: () => io, forDev: () => io } as unknown as SourceIOSet
  const backend = createLocalBackend({ store, blobStore: createIdbBlobStore(), sourceIO })
  return { store, backend }
}

afterEach(() => {
  run.jobs.length = 0
  run.results = {}
})

describe('isUndecodableError', () => {
  it('ブラウザが画像として開けなかったときだけ非対応。メモリ不足などは一時的', () => {
    expect(isUndecodableError(new DOMException('x', 'InvalidStateError'))).toBe(true)
    expect(isUndecodableError(new DOMException('x', 'EncodingError'))).toBe(true)
    expect(isUndecodableError(new DOMException('x', 'NotReadableError'))).toBe(false)
    expect(isUndecodableError(new RangeError('out of memory'))).toBe(false)
    expect(isUndecodableError('x')).toBe(false)
    expect(isUndecodableError(null)).toBe(false)
  })
})

describe('analyzePhotoFile の失敗の種類', () => {
  const original = globalThis.createImageBitmap
  afterEach(() => { globalThis.createImageBitmap = original })

  it('復号できない（InvalidStateError）は非対応、それ以外は一時的', async () => {
    globalThis.createImageBitmap = (async () => { throw new DOMException('decode', 'InvalidStateError') }) as never
    const broken = await analyzePhotoFile(new File(['x'], 'a.heic'))
    expect(broken.error).toBeTruthy()
    expect(broken.errorKind).toBe('unsupported')

    globalThis.createImageBitmap = (async () => { throw new RangeError('メモリ不足') }) as never
    const memory = await analyzePhotoFile(new File(['x'], 'b.jpg'))
    expect(memory.errorKind).toBe('transient')
  })
})

describe('行の判定（純粋）', () => {
  it('非対応・ハッシュ値だけ無い行は解析の対象にならず、種類の無い失敗と一時的は対象になる', () => {
    expect(needsAnalysis(row('a'))).toBe(true)
    expect(needsAnalysis(row('b', { analysisError: 'x' }))).toBe(true) // U58 より前の失敗は 1 回やり直す
    expect(needsAnalysis(row('c', { analysisError: 'x', analysisErrorKind: 'transient' }))).toBe(true)
    expect(needsAnalysis(unsupportedRow('d'))).toBe(false)
    expect(needsAnalysis(row('e', { analysisError: 'x', analysisErrorKind: 'unhashable' }))).toBe(false)
    expect(needsAnalysis(row('f', { dHash: 'ff' }))).toBe(false)
  })

  it('原本の大きさ・更新時刻が控えと同じなら据え置く', () => {
    const base = unsupportedRow('a')
    expect(isSettledUnsupported(base, { size: 10, lastModified: 1000 })).toBe(true)
    expect(isSettledUnsupported(base, { size: 11, lastModified: 1000 })).toBe(false)
    expect(isSettledUnsupported(base, { size: 10, lastModified: 2000 })).toBe(false)
    expect(isSettledUnsupported(row('t', { analysisError: 'x' }), { size: 10, lastModified: 1000 })).toBe(false)
  })

  it('選別の枚数から非対応と欠損を除く', () => {
    expect(selectableCount([row('a'), unsupportedRow('b'), row('c', { isMissing: true }), row('d', { analysisError: 'x' })])).toBe(2)
  })
})

describe('coreInputs は非対応を refs から外し、行は残す', () => {
  it('photos・byPath・byId には残り、refs（core の入力）には入らない', () => {
    const photo = (id: string, kind: 'unsupported' | 'transient' | null) => ({
      id, projectId: 'p', path: id, relativePath: id, name: id, capturedAt: 1, dHash: '00ff00ff00ff00ff',
      rating: id === 'b.jpg' ? 3 : 0, thumbnailPath: null, displayPath: null, analysisErrorKind: kind
    })
    const inputs = buildCoreInputs([photo('a.jpg', null), photo('b.jpg', 'unsupported'), photo('c.jpg', 'transient')])
    expect(inputs.refs.map(ref => ref.relative_path)).toEqual(['a.jpg', 'c.jpg'])
    expect(inputs.photos.map(item => item.relativePath)).toEqual(['a.jpg', 'b.jpg', 'c.jpg'])
    expect(inputs.byPath.get('b.jpg')?.rating).toBe(3)
    expect(inputs.byId.get('b.jpg')?.id).toBe('b.jpg')
  })
})

describe('Web の解析の再試行', () => {
  const files = {
    'same.jpg': { size: 10, modified: 1000 },
    'changed.jpg': { size: 99, modified: 1000 },
    'transient.jpg': { size: 5, modified: 1 },
    'legacy.jpg': { size: 5, modified: 1 },
    'fresh.jpg': { size: 5, modified: 1 },
    'nohash.jpg': { size: 5, modified: 1 },
    'ok.jpg': { size: 5, modified: 1 }
  }
  const rows = () => [
    unsupportedRow('same'),
    unsupportedRow('changed'),
    row('transient', { analysisError: '開けない', analysisErrorKind: 'transient' }),
    row('legacy', { analysisError: '古い失敗' }),
    row('fresh'),
    row('nohash', { analysisError: '小さい', analysisErrorKind: 'unhashable' })
  ]
  const ok = (extra: Partial<AnalyzedPhoto> = {}): AnalyzedPhoto => ({
    thumbnail: null, display: null, dHash: 'ff', capturedAt: 100, timestampSource: 'unknown', error: null, ...extra
  })

  it('非対応は原本が同じなら読まず、変わっていたら再試行する。一時的・種類の無い失敗は再試行する', async () => {
    const { backend } = await setup(files, rows())
    for (const id of ['changed', 'transient', 'legacy', 'fresh']) run.results[id] = ok()
    await backend.startBackgroundAnalysis('p')
    expect(run.jobs.map(job => job.id).sort()).toEqual(['changed', 'fresh', 'legacy', 'transient'])
    // 非対応の行は読んだ File をそのまま渡す（二度読まない）。
    expect(run.jobs.find(job => job.id === 'changed')?.hadFile).toBe(true)
    expect(run.jobs.find(job => job.id === 'same')).toBeUndefined()
    expect(run.jobs.find(job => job.id === 'nohash')).toBeUndefined()
  })

  it('非対応にした写真は、そのときの大きさ・更新時刻を控え、2 回目は読まない・数えない・選別の対象にしない', async () => {
    const { backend, store } = await setup(files, [row('fresh', { capturedAt: 1 }), row('ok', { capturedAt: 2 })])
    run.results.fresh = {
      thumbnail: null, display: null, dHash: null, capturedAt: 1, timestampSource: 'unknown',
      error: '画像を読み込めませんでした。', errorKind: 'unsupported'
    } satisfies AnalyzedPhoto
    run.results.ok = ok()
    const warnings: number[] = []
    backend.onProjectProgress?.(progress => warnings.push(progress.failed))
    await backend.startBackgroundAnalysis('p')
    const saved = (await store.photosOfProject('p')).find(item => item.id === 'fresh')!
    expect(saved).toMatchObject({
      analysisErrorKind: 'unsupported', analysisFailedSize: 5, analysisFailedModified: 1
    })
    expect(warnings.at(-1)).toBe(1)
    // 準備の分母（未処理）・枚数・一覧・core の入力。
    expect(await backend.getAnalysisBacklog('p')).toBe(0)
    expect((await backend.getAnalysisFailures('p')).map(item => [item.relativePath, item.kind]))
      .toEqual([['fresh.jpg', 'unsupported']])
    expect((await store.getProject('p'))?.photoCount).toBe(1)
    const inputs = buildCoreInputs(await backend.getCoreInputs('p'))
    expect(inputs.photos).toHaveLength(2)
    expect(inputs.refs.map(ref => ref.relative_path)).toEqual(['ok.jpg'])
    // 2 回目: 原本が変わっていないので、仕事は作られない。
    run.jobs.length = 0
    await backend.startBackgroundAnalysis('p')
    expect(run.jobs).toEqual([])
  })

  it('一時的な失敗は一覧に出て、未処理に数え、次に直る（B5）', async () => {
    const { backend, store } = await setup(files, [row('transient')])
    run.results.transient = {
      thumbnail: null, display: null, dHash: null, capturedAt: null, timestampSource: 'unknown',
      error: '開けなかった', errorKind: 'transient'
    } satisfies AnalyzedPhoto
    await backend.startBackgroundAnalysis('p')
    expect(await backend.getAnalysisBacklog('p')).toBe(1)
    expect((await backend.getAnalysisFailures('p'))[0]).toMatchObject({ kind: 'transient', reason: '開けなかった' })
    expect((await store.photosOfProject('p'))[0]).toMatchObject({ analysisErrorKind: 'transient', analysisFailedSize: null })
    // 次の回で直ったら、失敗が消える。
    run.results.transient = ok()
    await backend.startBackgroundAnalysis('p')
    expect(await backend.getAnalysisFailures('p')).toEqual([])
    expect((await store.photosOfProject('p'))[0]).toMatchObject({ analysisError: null, analysisErrorKind: null, dHash: 'ff' })
  })

  it('ハッシュ値だけ作れない写真は、数えず・一覧に出さず・選別の対象のまま', async () => {
    const { backend } = await setup(files, [row('nohash', { analysisError: '小さい', analysisErrorKind: 'unhashable' })])
    expect(await backend.getAnalysisBacklog('p')).toBe(0)
    expect(await backend.getAnalysisFailures('p')).toEqual([])
    expect((await backend.getCoreInputs('p'))[0]?.analysisErrorKind).toBeNull()
  })
})
