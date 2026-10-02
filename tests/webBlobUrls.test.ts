// @vitest-environment node
import 'fake-indexeddb/auto'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ObjectUrlCache } from '~/utils/objectUrlCache'
import { createIdbBlobStore } from '~/composables/backends/web/blobStore'
import { createIdbStore } from '~/composables/backends/web/store'
import { createLocalBackend } from '~/composables/backends/local'
import type { StoredPhoto, StoredProject } from '~/utils/browserStore'

/** revoke された URL を覚える。`createObjectURL` は本物（Node）を使う。 */
let revoked: Set<string>
let created: number
beforeEach(() => {
  revoked = new Set()
  created = 0
  const create = URL.createObjectURL.bind(URL)
  const revoke = URL.revokeObjectURL.bind(URL)
  vi.spyOn(URL, 'createObjectURL').mockImplementation(blob => {
    created += 1
    return create(blob)
  })
  vi.spyOn(URL, 'revokeObjectURL').mockImplementation(url => {
    revoked.add(url)
    revoke(url)
  })
})
afterEach(() => vi.restoreAllMocks())

const blobOf = (label: string) => new Blob([label], { type: 'image/jpeg' })

describe('ObjectUrlCache', () => {
  it('上限を超えると、いちばん古い URL から revoke される', () => {
    const cache = new ObjectUrlCache(64)
    const urls = Array.from({ length: 120 }, (_, i) => cache.get(`p${i}`, blobOf(`b${i}`)))
    expect(cache.size).toBe(64)
    expect(urls.slice(0, 56).every(url => revoked.has(url))).toBe(true)
    expect(urls.slice(56).some(url => revoked.has(url))).toBe(false)
  })
})

function photoRow(projectId: string, index: number): StoredPhoto {
  const name = `IMG_${String(index).padStart(4, '0')}.jpg`
  return {
    id: `${projectId}-${index}`, projectId, path: name, relativePath: name, name,
    capturedAt: 1_700_000_000_000 + index, timestampSource: 'exif_original', dHash: null,
    rating: 0, isMissing: false, analysisError: null
  }
}

function projectRow(id: string, count: number): StoredProject {
  return {
    id, name: id, source: { kind: 'folder', folderName: 'x' }, photoCount: count, status: 'ready',
    createdAt: 1, updatedAt: 1, burstThreshold: null, burstThresholdLearnedAt: null
  }
}

async function seed(projectId: string, count: number) {
  const store = createIdbStore()
  const blobStore = createIdbBlobStore()
  await store.putProject(projectRow(projectId, count))
  const rows = Array.from({ length: count }, (_, i) => photoRow(projectId, i))
  await store.putPhotos(rows)
  for (const row of rows) {
    await blobStore.put(row.id, { thumbnail: blobOf(`t-${row.id}`), display: blobOf(`d-${row.id}`) })
  }
  return { store, blobStore, rows }
}

describe('W3: 一覧の先頭を拡大しても URL が壊れない', () => {
  it('120 枚の一覧を読んだあと、先頭の拡大用 URL が revoke されていない', async () => {
    const projectId = 'w3-grid'
    const { store, blobStore } = await seed(projectId, 120)
    const backend = createLocalBackend({ store, blobStore })
    const page = await backend.getProjectPhotoPage(projectId, 0, 120)
    expect(page.photos).toHaveLength(120)
    const first = page.photos[0]!
    const zoom = await backend.photoOriginalUrl(first)
    expect(zoom).toBeTruthy()
    expect(revoked.has(zoom)).toBe(false)
    // 表示用の Blob を 120 個ぶん読んで URL にしない（一覧のタイルはサムネイルだけ）。
    expect(created).toBeLessThanOrEqual(120 + 1)
  })

  it('選別用の getPhotosByIds は表示用 URL を返す', async () => {
    const projectId = 'w3-ids'
    const { store, blobStore, rows } = await seed(projectId, 5)
    const backend = createLocalBackend({ store, blobStore })
    const photos = await backend.getPhotosByIds(projectId, [rows[2]!.id, rows[0]!.id])
    expect(photos.map(photo => photo.id)).toEqual([rows[2]!.id, rows[0]!.id])
    expect(photos[0]!.displayPath).toMatch(/^blob:/)
  })
})
