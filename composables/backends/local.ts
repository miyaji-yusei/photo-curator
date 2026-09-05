/**
 * ブラウザ（iPad など）向けの実装。端末の中だけで完結する。
 *
 * - 写真は写真ピッカーから受け取る。**原本は保存しない**（iOS には永続的な
 *   ファイルハンドルが無いため、リロードすると参照が切れる）。
 *   代わりに 256px のサムネイルと星を IndexedDB に残すので、
 *   リロード後も選別を続けられる。拡大だけは再取り込みまで 256px になる。
 * - 解析（撮影日時・サムネイル・dHash）はブラウザの復号器で行う。
 *   Safari は HEIC も読めるので、iPad の写真がそのまま扱える。
 * - **写真ライブラリは書き換えない。** 反映は共有シートや ZIP 書き出しなど、
 *   利用者の操作を経由する。
 */
import type {
  BurstGroup, BurstPair, ExportReport, Photo, PhotoPage, PhotoSort, Project,
  ProjectProgress, ProjectTask, SelectionResult, SelectionSeed, SelectionSession, SelectionSummary
} from '~/types/photo'
import { MAX_RATING } from '~/types/photo'
import type { PhotoBackend } from '~/composables/photoBackend'
import { normalizeSession } from '~/utils/tournament'
import { analyzeAll } from '~/utils/analysisPool'
import {
  BURST_WINDOW_MS, HASH_DISTANCE_LIMIT, buildBurstGroups, buildBurstPairs,
  pairJoinsByThreshold, pairKey
} from '~/utils/burstAnalysis'
import type { BurstEntry } from '~/utils/burstAnalysis'
import {
  STORE_PHOTOS, STORE_PROJECTS, STORE_STATES, STORE_THUMBNAILS,
  deleteOne, getAll, getOne, photosOfProject, putOne, readPairOverrides, readSession,
  requestPersistence, toPhoto, withStores, writePairOverrides, writeSession
} from '~/utils/browserStore'
import type { StoredPhoto, StoredProject } from '~/utils/browserStore'
import { ObjectUrlCache } from '~/utils/objectUrlCache'
import {
  compareByCaptureOrder, comparePhotos, filterPhotos, pagePhotos, summarizeRatings
} from '~/utils/photoQuery'

/** 原本はセッション中だけ持つ。リロードで消えるが、保存もしない。 */
const originals = new Map<string, File>()
const thumbnailUrls = new ObjectUrlCache()
const originalUrls = new ObjectUrlCache(64)

const listeners = new Set<(progress: ProjectProgress) => void>()
const cancelled = new Set<string>()

function emit(progress: ProjectProgress) {
  for (const listener of listeners) listener(progress)
}

const progressOf = (
  projectId: string, task: ProjectTask, phase: ProjectProgress['phase'],
  processed: number, total: number, message: string, failed = 0
): ProjectProgress => ({ projectId, task, phase, processed, total, message, warning: null, failed })

function asProject(row: StoredProject): Project {
  return {
    id: row.id,
    name: row.name,
    // ブラウザにフォルダの概念は無い。画面には取り込み元の説明を出す。
    folderPath: 'この iPad の写真',
    photoCount: row.photoCount,
    status: row.status,
    createdAt: row.createdAt,
    updatedAt: row.updatedAt,
    burstThreshold: row.burstThreshold,
    burstThresholdLearnedAt: row.burstThresholdLearnedAt
  }
}

/** 行に URL を付けて画面が使える形にする。サムネイルは 1 枚ずつ読む。 */
async function decorate(rows: StoredPhoto[]): Promise<Photo[]> {
  const blobs = await withStores([STORE_THUMBNAILS], 'readonly', async transaction => {
    const found = new Map<string, Blob>()
    for (const row of rows) {
      const stored = await getOne<{ photoId: string, blob: Blob }>(transaction, STORE_THUMBNAILS, row.id)
      if (stored?.blob) found.set(row.id, stored.blob)
    }
    return found
  })
  return rows.map(row => {
    const blob = blobs.get(row.id)
    const thumbnailUrl = blob ? thumbnailUrls.get(row.id, blob) : null
    const file = originals.get(row.id)
    const originalUrl = file ? originalUrls.get(row.id, file) : null
    return toPhoto(row, thumbnailUrl, originalUrl)
  })
}

/** 連写判定に使える写真だけを撮影順で取り出す。 */
function burstEntries(rows: StoredPhoto[]): BurstEntry[] {
  return rows
    .filter(row => !row.isMissing && row.capturedAt !== null && row.dHash !== null)
    .sort(compareByCaptureOrder)
    .map(row => ({
      id: row.id,
      capturedAt: row.capturedAt!,
      dHash: row.dHash!,
      source: row.timestampSource
    }))
}

async function loadProject(projectId: string): Promise<StoredProject | undefined> {
  return withStores([STORE_PROJECTS], 'readonly', transaction =>
    getOne<StoredProject>(transaction, STORE_PROJECTS, projectId)
  )
}

async function patchProject(projectId: string, patch: Partial<StoredProject>) {
  await withStores([STORE_PROJECTS], 'readwrite', async transaction => {
    const row = await getOne<StoredProject>(transaction, STORE_PROJECTS, projectId)
    if (!row) return
    await putOne(transaction, STORE_PROJECTS, { ...row, ...patch, updatedAt: Date.now() })
  })
}

const unsupported = (what: string) =>
  Promise.reject(new Error(`${what}はこの端末では行えません。ブラウザから写真ライブラリを書き換えられないためです。`))

export function createLocalBackend(): PhotoBackend {
  return {
    kind: 'local',
    capabilities: capabilitiesFor('browser'),
    // フォルダを走査できるのはデスクトップだけ。
    chooseFolder: () => Promise.resolve(null),

    onProjectProgress: (callback) => {
      listeners.add(callback)
      return Promise.resolve(() => listeners.delete(callback))
    },

    listProjects: async () => {
      const rows = await withStores([STORE_PROJECTS], 'readonly', transaction =>
        getAll<StoredProject>(transaction, STORE_PROJECTS)
      )
      return rows
        .sort((left, right) => right.updatedAt - left.updatedAt)
        .map(asProject)
    },

    createProject: async (name: string) => {
      const now = Date.now()
      const row: StoredProject = {
        id: crypto.randomUUID(),
        name: name.trim() || '新しいプロジェクト',
        photoCount: 0,
        status: 'new',
        createdAt: now,
        updatedAt: now,
        burstThreshold: null,
        burstThresholdLearnedAt: null
      }
      await withStores([STORE_PROJECTS], 'readwrite', transaction =>
        putOne(transaction, STORE_PROJECTS, row)
      )
      return asProject(row)
    },

    deleteProject: async (projectId: string) => {
      const rows = await withStores([STORE_PHOTOS], 'readonly', transaction =>
        photosOfProject(transaction, projectId)
      )
      await withStores(
        [STORE_PROJECTS, STORE_PHOTOS, STORE_THUMBNAILS, STORE_STATES], 'readwrite',
        async transaction => {
          for (const row of rows) {
            thumbnailUrls.release(row.id)
            originalUrls.release(row.id)
            originals.delete(row.id)
            await deleteOne(transaction, STORE_PHOTOS, row.id)
            await deleteOne(transaction, STORE_THUMBNAILS, row.id)
          }
          await deleteOne(transaction, STORE_STATES, projectId)
          await deleteOne(transaction, STORE_PROJECTS, projectId)
        }
      )
    },

    getProjectPhotoPage: async (
      projectId: string, offset = 0, limit = 80,
      rating: number | null = null, sort: PhotoSort = 'name'
    ): Promise<PhotoPage> => {
      const rows = await withStores([STORE_PHOTOS], 'readonly', transaction =>
        photosOfProject(transaction, projectId)
      )
      const matched = filterPhotos(rows, rating).sort(comparePhotos(sort))
      // 画面に渡すのは 1 ページぶんだけ。全件を載せない。
      return { photos: await decorate(pagePhotos(matched, offset, limit)), total: matched.length }
    },

    getPhotosByIds: async (projectId: string, photoIds: string[]) => {
      const rows = await withStores([STORE_PHOTOS], 'readonly', transaction =>
        photosOfProject(transaction, projectId)
      )
      const byId = new Map(rows.map(row => [row.id, row]))
      // 渡された順を保つ。まとめの代表が先頭に来る前提の画面がある。
      const ordered = photoIds.map(id => byId.get(id)).filter((row): row is StoredPhoto => !!row)
      return decorate(ordered)
    },

    getSelectionSeed: async (projectId: string, rating?: number): Promise<SelectionSeed[]> => {
      const rows = await withStores([STORE_PHOTOS], 'readonly', transaction =>
        photosOfProject(transaction, projectId)
      )
      // 選別の並びは撮影順。似た構図が隣り合うようにする。
      return filterPhotos(rows, rating ?? null)
        .sort(compareByCaptureOrder)
        .map(row => ({ id: row.id, rating: row.rating }))
    },

    saveSelectionResults: async (projectId: string, entries: SelectionResult[]) => {
      if (!entries.length) return
      await withStores([STORE_PHOTOS], 'readwrite', async transaction => {
        for (const entry of entries) {
          const row = await getOne<StoredPhoto>(transaction, STORE_PHOTOS, entry.id)
          if (!row || row.projectId !== projectId) continue
          await putOne(transaction, STORE_PHOTOS, {
            ...row,
            rating: Math.min(MAX_RATING, Math.max(0, entry.rating))
          })
        }
      })
    },

    resetSelectionResults: async (projectId: string) => {
      await withStores([STORE_PHOTOS], 'readwrite', async transaction => {
        const rows = await photosOfProject(transaction, projectId)
        for (const row of rows) {
          if (row.rating === 0) continue
          await putOne(transaction, STORE_PHOTOS, { ...row, rating: 0 })
        }
      })
    },

    moveRating: async (
      projectId: string, fromRating: number, toRating: number,
      includeIds: string[] | null = null, excludeIds: string[] = []
    ) => {
      const valid = (rating: number) => rating >= 0 && rating <= MAX_RATING
      if (!valid(fromRating) || !valid(toRating)) {
        throw new Error(`レートは 0〜${MAX_RATING} の範囲で指定してください。`)
      }
      // 同じ星への移動は操作として意味が無い。何もしない。
      if (fromRating === toRating) return 0

      const include = includeIds === null ? null : new Set(includeIds)
      const exclude = new Set(excludeIds)
      return withStores([STORE_PHOTOS], 'readwrite', async transaction => {
        const rows = await photosOfProject(transaction, projectId)
        let moved = 0
        for (const row of rows) {
          if (row.isMissing || row.rating !== fromRating) continue
          if (include ? !include.has(row.id) : exclude.has(row.id)) continue
          await putOne(transaction, STORE_PHOTOS, { ...row, rating: toRating })
          moved += 1
        }
        return moved
      })
    },

    getSelectionSummary: async (projectId: string): Promise<SelectionSummary> => {
      const rows = await withStores([STORE_PHOTOS], 'readonly', transaction =>
        photosOfProject(transaction, projectId)
      )
      return summarizeRatings(rows)
    },

    getBurstGroups: async (projectId: string, threshold?: number): Promise<BurstGroup[]> => {
      const [rows, project, overrides] = await Promise.all([
        withStores([STORE_PHOTOS], 'readonly', transaction => photosOfProject(transaction, projectId)),
        loadProject(projectId),
        readPairOverrides(projectId)
      ])
      const limit = threshold ?? project?.burstThreshold ?? HASH_DISTANCE_LIMIT
      return buildBurstGroups(burstEntries(rows), limit, overrides)
    },

    getBurstNeighborhood: async (
      projectId: string, photoIds: string[], windowMs = BURST_WINDOW_MS
    ): Promise<Photo[]> => {
      if (!photoIds.length) return []
      const rows = await withStores([STORE_PHOTOS], 'readonly', transaction =>
        photosOfProject(transaction, projectId)
      )
      // まとめの判定に使える写真だけを対象にする。デスクトップ側と同じ絞り込み。
      const usable = rows.filter(
        row => !row.isMissing && row.capturedAt !== null && row.dHash !== null
      )
      const target = new Set(photoIds)
      const times = usable.filter(row => target.has(row.id)).map(row => row.capturedAt!)
      if (!times.length) return []
      const low = Math.min(...times) - Math.max(0, windowMs)
      const high = Math.max(...times) + Math.max(0, windowMs)
      return decorate(
        usable
          .filter(row => row.capturedAt! >= low && row.capturedAt! <= high)
          .sort(compareByCaptureOrder)
      )
    },

    saveBurstShape: async (
      projectId: string, orderedPhotoIds: string[], blocks: string[][]
    ): Promise<void> => {
      if (orderedPhotoIds.length < 2) return
      const [rows, project, overrides] = await Promise.all([
        withStores([STORE_PHOTOS], 'readonly', transaction => photosOfProject(transaction, projectId)),
        loadProject(projectId),
        readPairOverrides(projectId)
      ])
      const threshold = project?.burstThreshold ?? HASH_DISTANCE_LIMIT
      const byId = new Map(burstEntries(rows).map(entry => [entry.id, entry]))
      const blockOf = new Map<string, number>()
      blocks.forEach((block, index) => {
        for (const id of block) blockOf.set(id, index)
      })
      // 閾値だけで出る素の判定と食い違うペアだけを残す。一致するものは消すので、
      // 切ってから元に戻しても無意味な例外が溜まらない（デスクトップと同じ規則）。
      for (let index = 0; index + 1 < orderedPhotoIds.length; index += 1) {
        const left = byId.get(orderedPhotoIds[index]!)
        const right = byId.get(orderedPhotoIds[index + 1]!)
        if (!left || !right) continue
        const raw = pairJoinsByThreshold(left, right, threshold)
        const leftBlock = blockOf.get(left.id)
        const wanted = leftBlock !== undefined && leftBlock === blockOf.get(right.id)
        const key = pairKey(left.id, right.id)
        if (wanted === raw) overrides.delete(key)
        else overrides.set(key, wanted)
      }
      await writePairOverrides(projectId, overrides)
    },

    getBurstPairs: async (projectId: string): Promise<BurstPair[]> => {
      const rows = await withStores([STORE_PHOTOS], 'readonly', transaction =>
        photosOfProject(transaction, projectId)
      )
      return buildBurstPairs(burstEntries(rows))
    },

    saveBurstThreshold: (projectId: string, threshold: number) =>
      patchProject(projectId, { burstThreshold: threshold, burstThresholdLearnedAt: Date.now() }),

    clearBurstThreshold: (projectId: string) =>
      patchProject(projectId, { burstThreshold: null, burstThresholdLearnedAt: null }),

    // 取り込みのときに解析まで済ませるので、あとから走らせる処理は無い。
    startProjectScan: () => Promise.resolve(),
    startBurstAnalysis: () => Promise.resolve(),
    startBackgroundAnalysis: () => Promise.resolve(),

    getAnalysisBacklog: async (projectId: string) => {
      const rows = await withStores([STORE_PHOTOS], 'readonly', transaction =>
        photosOfProject(transaction, projectId)
      )
      return rows.filter(row => row.dHash === null && row.analysisError === null).length
    },

    cancelProjectTask: (projectId: string) => {
      cancelled.add(projectId)
      return Promise.resolve()
    },

    saveSession: (session: SelectionSession) => writeSession(session),
    loadSession: async (projectId: string) => {
      const raw = await readSession(projectId)
      if (!raw) return null
      return normalizeSession(JSON.parse(raw) as SelectionSession)
    },

    // ブラウザでは既に表示できる URL が入っている。
    photoUrl: (path: string) => path,
    photoThumbnailUrl: (photo: Photo) => photo.thumbnailPath ?? photo.path,

    exportByRating: (): Promise<ExportReport> => unsupported('フォルダ分け'),
    writeRatingsToFiles: (): Promise<ExportReport> => unsupported('メタデータへの書き込み'),

    /**
     * 写真ピッカーで選ばれたファイルを取り込み、そのまま解析する。
     * 1 枚ごとに書くので、途中で閉じてもそこまでは残る。
     */
    importPhotos: async (projectId: string, files: File[]) => {
      if (!files.length) return
      cancelled.delete(projectId)
      await requestPersistence()

      const total = files.length
      emit(progressOf(projectId, 'scan', 'indexing', 0, total, '写真を読み込んでいます'))

      const jobs = files.map(file => ({ id: crypto.randomUUID(), file }))
      await withStores([STORE_PHOTOS], 'readwrite', async transaction => {
        for (const [index, job] of jobs.entries()) {
          originals.set(job.id, job.file)
          const row: StoredPhoto = {
            id: job.id,
            projectId,
            path: job.file.name,
            relativePath: job.file.webkitRelativePath || job.file.name,
            name: job.file.name,
            capturedAt: null,
            timestampSource: 'unknown',
            dHash: null,
            rating: 0,
            isMissing: false,
            analysisError: null
          }
          await putOne(transaction, STORE_PHOTOS, row)
          if (index % 50 === 0) {
            emit(progressOf(projectId, 'scan', 'indexing', index, total, '写真を読み込んでいます'))
          }
        }
      })

      const existing = await withStores([STORE_PHOTOS], 'readonly', transaction =>
        photosOfProject(transaction, projectId)
      )
      await patchProject(projectId, { photoCount: existing.length, status: 'ready' })
      emit(progressOf(projectId, 'scan', 'complete', total, total, '読み込みが終わりました'))

      let processed = 0
      let failed = 0
      emit(progressOf(projectId, 'background', 'hashing', 0, total, '写真を解析しています'))
      await analyzeAll(jobs, {
        isCancelled: () => cancelled.has(projectId),
        onResult: async (id, analyzed) => {
          processed += 1
          if (analyzed.error) failed += 1
          await withStores([STORE_PHOTOS, STORE_THUMBNAILS], 'readwrite', async transaction => {
            const row = await getOne<StoredPhoto>(transaction, STORE_PHOTOS, id)
            if (!row) return
            await putOne(transaction, STORE_PHOTOS, {
              ...row,
              capturedAt: analyzed.capturedAt,
              timestampSource: analyzed.timestampSource,
              dHash: analyzed.dHash,
              analysisError: analyzed.error
            })
            if (analyzed.thumbnail) {
              await putOne(transaction, STORE_THUMBNAILS, { photoId: id, blob: analyzed.thumbnail })
            }
          })
          emit(progressOf(projectId, 'background', 'hashing', processed, total, '写真を解析しています', failed))
        }
      })

      const phase = cancelled.has(projectId) ? 'cancelled' : 'complete'
      emit(progressOf(projectId, 'background', phase, processed, total, '解析が終わりました', failed))
      await patchProject(projectId, {})
    },

    /** 共有・書き出しに使える原本。リロード後は空になる。 */
    originalFile: (photoId: string) => originals.get(photoId) ?? null
  }
}
