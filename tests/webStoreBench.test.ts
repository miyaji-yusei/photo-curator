// @vitest-environment node
/**
 * Web の行の読み（W2）の小さなベンチ。行数 5,000 のダミーデータで、1 タップや一覧で呼ばれる
 * 読みの所要時間を `console.log` に出す。時間そのものは検証しない（環境で変わるため）。
 * 検証するのは**返す値と並び**。
 */
import 'fake-indexeddb/auto'
import { describe, expect, it } from 'vitest'
import { createIdbStore } from '~/composables/backends/web/store'
import { createIdbBlobStore } from '~/composables/backends/web/blobStore'
import { createLocalBackend } from '~/composables/backends/local'
import type { StoredPhoto, StoredProject } from '~/utils/browserStore'

const ROWS = 5_000
const PROJECT = 'bench-project'

function row(index: number): StoredPhoto {
  // 並びが「挿入順」と違うよう、鍵をばらす。
  const name = `IMG_${String((index * 7919) % ROWS).padStart(5, '0')}.jpg`
  return {
    id: `p-${index}`, projectId: PROJECT, path: name, relativePath: `a/${name}`, name,
    capturedAt: index % 50 === 0 ? null : 1_700_000_000_000 + index * 1000, timestampSource: 'exif_original',
    dHash: index % 3 === 0 ? null : 'abcdef0123456789', rating: index % 6, isMissing: index % 97 === 0,
    analysisError: null
  }
}

const project: StoredProject = {
  id: PROJECT, name: 'bench', source: { kind: 'folder', folderName: 'x' }, photoCount: ROWS, status: 'ready',
  createdAt: 1, updatedAt: 1, burstThreshold: null, burstThresholdLearnedAt: null
}

async function time(label: string, repeat: number, body: () => Promise<unknown>) {
  await body()
  const start = performance.now()
  for (let i = 0; i < repeat; i += 1) await body()
  const ms = (performance.now() - start) / repeat
  console.log(`[bench] ${label}: ${ms.toFixed(2)} ms/回（${repeat} 回の平均）`)
  return ms
}

describe('W2 ベンチ（行数 5,000）', () => {
  it('読みの所要時間と、返す値・並び', async () => {
    const store = createIdbStore()
    await store.putProject(project)
    await store.putPhotos(Array.from({ length: ROWS }, (_, i) => row(i)))
    const backend = createLocalBackend({ store, blobStore: createIdbBlobStore() })

    const ids = Array.from({ length: 10 }, (_, i) => `p-${i * 400 + 3}`).reverse()
    await time('getPhotosByIds(10 枚)', 30, () => backend.getPhotosByIds(PROJECT, ids))
    await time('getSelectionSummary', 30, () => backend.getSelectionSummary(PROJECT))
    await time('getProjectPhotoPage(0, 1)（ホームの見本）', 30, () => backend.getProjectPhotoPage(PROJECT, 0, 1))
    await time('getProjectPhotoPage(80, 80, rating 3)', 30, () => backend.getProjectPhotoPage(PROJECT, 80, 80, 3))
    await time('getAnalysisBacklog', 30, () => backend.getAnalysisBacklog(PROJECT))
    await time('getCoreInputs', 10, () => backend.getCoreInputs(PROJECT))

    // 返す値・並び。
    const got = await backend.getPhotosByIds(PROJECT, [...ids, 'nope', ids[0]!])
    expect(got.map(photo => photo.id)).toEqual([...ids, ids[0]])
    const summary = await backend.getSelectionSummary(PROJECT)
    expect(summary.total).toBe(Array.from({ length: ROWS }, (_, i) => row(i)).filter(r => !r.isMissing).length)
    const page = await backend.getProjectPhotoPage(PROJECT, 0, 5)
    const expected = Array.from({ length: ROWS }, (_, i) => row(i))
      .filter(r => !r.isMissing).sort((a, b) => (a.relativePath < b.relativePath ? -1 : 1)).slice(0, 5)
    expect(page.photos.map(photo => photo.id)).toEqual(expected.map(r => r.id))
    const byRating = await backend.getProjectPhotoPage(PROJECT, 0, 5, 3, 'rating')
    expect(byRating.photos.every(photo => photo.rating === 3)).toBe(true)
  }, 60_000)
})

describe('W2 行キャッシュ: 書き込みの後の読みが古くならない', () => {
  async function fresh(id: string, count: number) {
    const store = createIdbStore()
    await store.putProject({ ...project, id, photoCount: count })
    await store.putPhotos(Array.from({ length: count }, (_, i) => ({
      ...row(i), id: `${id}-${i}`, projectId: id, isMissing: false, rating: 0
    })))
    return store
  }

  it('星の保存・移動・リセット・追加・patch・削除のあと、読みに反映される', async () => {
    const store = await fresh('cache-a', 20)
    const ratingOf = async () => (await store.photosOfProject('cache-a')).map(r => r.rating)
    expect((await ratingOf()).every(r => r === 0)).toBe(true) // 覚える

    await store.saveSelectionResults('cache-a', [{ id: 'cache-a-1', rating: 5 }, { id: 'cache-a-2', rating: 4 }, { id: 'other', rating: 3 }])
    const afterSave = await store.photosOfProject('cache-a')
    expect(afterSave.find(r => r.id === 'cache-a-1')!.rating).toBe(5)
    expect(afterSave.find(r => r.id === 'cache-a-2')!.rating).toBe(4)

    expect(await store.moveRating('cache-a', 5, 2, null, new Set())).toBe(1)
    expect((await store.photosOfProject('cache-a')).find(r => r.id === 'cache-a-1')!.rating).toBe(2)

    await store.patchPhoto('cache-a-3', { dHash: 'ffff', analysisError: 'x' })
    const patched = (await store.photosOfProject('cache-a')).find(r => r.id === 'cache-a-3')!
    expect([patched.dHash, patched.analysisError]).toEqual(['ffff', 'x'])

    await store.resetRatings('cache-a')
    expect((await ratingOf()).every(r => r === 0)).toBe(true)

    await store.putPhotos([{ ...row(99), id: 'cache-a-new', projectId: 'cache-a', isMissing: false }])
    expect(await store.photosOfProject('cache-a')).toHaveLength(21)

    await store.deleteProject('cache-a', (await store.photosOfProject('cache-a')).map(r => r.id))
    expect(await store.photosOfProject('cache-a')).toHaveLength(0)
  })

  it('返した配列を並べ替えても、覚えた側の並びは変わらない', async () => {
    const store = await fresh('cache-b', 10)
    const first = await store.photosOfProject('cache-b')
    const original = first.map(r => r.id)
    first.reverse()
    first.length = 0
    expect((await store.photosOfProject('cache-b')).map(r => r.id)).toEqual(original)
  })

  it('読みの途中に書き込みが入っても、古い結果を覚えない', async () => {
    const store = await fresh('cache-c', 10)
    const reading = store.photosOfProject('cache-c')
    const writing = store.saveSelectionResults('cache-c', [{ id: 'cache-c-0', rating: 5 }])
    await Promise.all([reading, writing])
    expect((await store.photosOfProject('cache-c')).find(r => r.id === 'cache-c-0')!.rating).toBe(5)
  })

  it('photosByIds は渡した順・重複のまま返し、無い id と別のプロジェクトの行は undefined', async () => {
    const store = await fresh('cache-d', 5)
    await fresh('cache-e', 2)
    const got = await store.photosByIds('cache-d', ['cache-d-3', 'cache-d-1', 'cache-d-3', 'nope', 'cache-e-0'])
    expect(got.map(r => r?.id)).toEqual(['cache-d-3', 'cache-d-1', 'cache-d-3', undefined, undefined])
  })
})
