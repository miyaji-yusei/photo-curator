/**
 * プロジェクト・写真の行・状態（Session）・手直しの読み書き（IndexedDB）。
 *
 * 画像の実体は `blobStore.ts`、原本のバイトは `sourceIO.ts`。ここは**行だけ**を持つ。
 * `createLocalBackend` はこの口だけを見るので、別の保存先（Swift の殻など）に差し替えられる。
 *
 * ## 保存の置き場（`photo-curator-mb`）
 * - `projects` / `photos`（索引 projectId）… 行
 * - `states` … Session（キー `<projectId>`）とフォルダの handle（キー `handle:<projectId>`）
 * - `burstShapes` … 手で直した連写の例外（キー `overrides:<projectId>`）
 * - `states` には、サイドカーの端末の記憶（キー `sidecar:<projectId>`）と端末の id（キー `device`）、
 *   Amazon の tempLink（キー `amazonLinks:<projectId>`）も置く
 */
import type { PairOverride } from '~/lib/core'
import type { SelectionResult } from '~/types/photo'
import type { SidecarState } from '~/composables/photoBackend'
import { randomUUID } from '~/utils/uuid'
import { MAX_RATING } from '~/types/photo'
import {
  STORE_BURST_SHAPES, STORE_PHOTOS, STORE_PROJECTS, STORE_STATES,
  deleteOne, getAll, getOne, photosOfProject, putOne, withStores
} from '~/utils/browserStore'
import type { StoredPhoto, StoredProject } from '~/utils/browserStore'

export interface WebStore {
  listProjects: () => Promise<StoredProject[]>
  getProject: (projectId: string) => Promise<StoredProject | undefined>
  putProject: (row: StoredProject) => Promise<void>
  patchProject: (projectId: string, patch: Partial<StoredProject>) => Promise<void>
  /** 行・Session・手直し・handle を消す。画像の実体は `blobStore.remove`。 */
  deleteProject: (projectId: string, photoIds: string[]) => Promise<void>

  /**
   * そのプロジェクトの全行。**読んだ結果をプロジェクトごとに覚える**（書き込みの経路は全部、覚えたものを
   * 更新か破棄する）。返す配列は呼び出しごとの写しなので、並べ替えても覚えた側は変わらない。
   * 他のタブの書き込みは見えない（このページ 1 つが書き手という前提）。
   */
  photosOfProject: (projectId: string) => Promise<StoredPhoto[]>
  /**
   * 主キーで読む。渡した順・重複のまま返し、無い id・別のプロジェクトの行は `undefined`。
   * 全行を読まないので、組の表示（10〜20 枚）に使う。
   */
  photosByIds: (projectId: string, photoIds: string[]) => Promise<(StoredPhoto | undefined)[]>
  putPhotos: (rows: StoredPhoto[]) => Promise<void>
  /** 1 枚の行だけを読み直して書く。無ければ何もしない。 */
  patchPhoto: (photoId: string, patch: Partial<StoredPhoto>) => Promise<void>
  saveSelectionResults: (projectId: string, entries: SelectionResult[]) => Promise<void>
  resetRatings: (projectId: string) => Promise<void>
  moveRating: (
    projectId: string, fromRating: number, toRating: number, include: Set<string> | null, exclude: Set<string>
  ) => Promise<number>

  readSession: (projectId: string) => Promise<string | null>
  writeSession: (projectId: string, stateJson: string) => Promise<void>
  readPairOverrides: (projectId: string) => Promise<PairOverride[]>
  writePairOverrides: (projectId: string, overrides: PairOverride[]) => Promise<void>

  /** サイドカーで端末が覚える 3 つの値。まだ無ければ「未確認・変更なし」。 */
  readSidecarState: (projectId: string) => Promise<SidecarState>
  writeSidecarState: (projectId: string, state: SidecarState) => Promise<void>
  /** 表示用画像の長辺の既定（アプリ全体）。まだ保存が無ければ null。 */
  readDisplayEdge: () => Promise<number | null>
  writeDisplayEdge: (edge: number) => Promise<void>
  /** この端末の id。初回に作って残す。 */
  deviceId: () => Promise<string>

  /** Amazon の tempLink（node id → URL）。無ければ null。 */
  readAmazonLinks: (projectId: string) => Promise<Record<string, string> | null>
  writeAmazonLinks: (projectId: string, links: Record<string, string>) => Promise<void>

  readHandle: (projectId: string) => Promise<FileSystemDirectoryHandle | null>
  writeHandle: (projectId: string, handle: FileSystemDirectoryHandle) => Promise<void>
}

/** 手で直した連写の例外（core の `PairOverride`、鍵は relativePath）を置く `burstShapes` のキー。 */
export const overridesKey = (projectId: string) => `overrides:${projectId}`
/** フォルダの handle を置く `states` のキー。 */
export const handleKey = (projectId: string) => `handle:${projectId}`
/** Amazon の tempLink を置く `states` のキー。 */
export const amazonLinksKey = (projectId: string) => `amazonLinks:${projectId}`
/** サイドカーの端末の記憶を置く `states` のキー。 */
export const sidecarStateKey = (projectId: string) => `sidecar:${projectId}`
/** 端末の id を置く `states` のキー。 */
export const DEVICE_KEY = 'device'
/** 表示用画像の長辺の既定を置く `states` のキー。 */
export const DISPLAY_EDGE_KEY = 'displayEdge'

export function createIdbStore(): WebStore {
  /** プロジェクトごとの全行。書き込みで更新か破棄する。 */
  const rowCache = new Map<string, StoredPhoto[]>()
  /** 書き込みのたびに進める。読みの途中で書き込みが入ったら、その読みの結果は覚えない。 */
  let writeGeneration = 0
  const dropRows = (projectId: string) => {
    writeGeneration += 1
    rowCache.delete(projectId)
  }
  /** 覚えた全行へ、書き込みの結果をそのまま反映する（書き込みの完了後に呼ぶ）。 */
  const patchCachedRows = (projectId: string, changes: Map<string, StoredPhoto>) => {
    writeGeneration += 1
    const cached = rowCache.get(projectId)
    if (!cached) return
    rowCache.set(projectId, cached.map(row => changes.get(row.id) ?? row))
  }

  return {
    listProjects: () =>
      withStores([STORE_PROJECTS], 'readonly', transaction => getAll<StoredProject>(transaction, STORE_PROJECTS)),

    getProject: projectId =>
      withStores([STORE_PROJECTS], 'readonly', transaction =>
        getOne<StoredProject>(transaction, STORE_PROJECTS, projectId)),

    putProject: async row => {
      await withStores([STORE_PROJECTS], 'readwrite', transaction => putOne(transaction, STORE_PROJECTS, row))
    },

    patchProject: async (projectId, patch) => {
      await withStores([STORE_PROJECTS], 'readwrite', async transaction => {
        const row = await getOne<StoredProject>(transaction, STORE_PROJECTS, projectId)
        if (!row) return
        await putOne(transaction, STORE_PROJECTS, { ...row, ...patch, updatedAt: Date.now() })
      })
    },

    deleteProject: async (projectId, photoIds) => {
      await withStores(
        [STORE_PROJECTS, STORE_PHOTOS, STORE_STATES, STORE_BURST_SHAPES], 'readwrite',
        async transaction => {
          for (const id of photoIds) await deleteOne(transaction, STORE_PHOTOS, id)
          await deleteOne(transaction, STORE_STATES, projectId)
          await deleteOne(transaction, STORE_STATES, handleKey(projectId))
          await deleteOne(transaction, STORE_STATES, sidecarStateKey(projectId))
          await deleteOne(transaction, STORE_STATES, amazonLinksKey(projectId))
          await deleteOne(transaction, STORE_BURST_SHAPES, overridesKey(projectId))
          await deleteOne(transaction, STORE_PROJECTS, projectId)
        }
      )
      dropRows(projectId)
    },

    photosOfProject: async projectId => {
      const cached = rowCache.get(projectId)
      if (cached) return cached.slice()
      const generation = writeGeneration
      const rows = await withStores([STORE_PHOTOS], 'readonly', transaction => photosOfProject(transaction, projectId))
      // 読んでいる間に書き込みが入っていたら、古いかもしれないので覚えない。
      if (generation === writeGeneration) rowCache.set(projectId, rows)
      return rows.slice()
    },

    photosByIds: async (projectId, photoIds) => {
      // リクエストを先に全部出してから待つ（1 件ずつ往復しない）。
      const rows = await withStores([STORE_PHOTOS], 'readonly', transaction =>
        Promise.all(photoIds.map(id => getOne<StoredPhoto>(transaction, STORE_PHOTOS, id))))
      return rows.map(row => (row && row.projectId === projectId ? row : undefined))
    },

    putPhotos: async rows => {
      if (!rows.length) return
      try {
        await withStores([STORE_PHOTOS], 'readwrite', transaction => {
          const photos = transaction.objectStore(STORE_PHOTOS)
          // 1 件ずつ待たない。トランザクションの完了が全部の書き込みの完了。
          for (const row of rows) photos.put(row)
        })
      } finally {
        for (const projectId of new Set(rows.map(row => row.projectId))) dropRows(projectId)
      }
    },

    patchPhoto: async (photoId, patch) => {
      const written: StoredPhoto[] = []
      try {
        await withStores([STORE_PHOTOS], 'readwrite', async transaction => {
          const row = await getOne<StoredPhoto>(transaction, STORE_PHOTOS, photoId)
          if (!row) return
          const next = { ...row, ...patch }
          await putOne(transaction, STORE_PHOTOS, next)
          written.push(next)
        })
      } catch (cause) {
        // 書けたかどうか分からないので、覚えたものは捨てる。
        for (const row of written) dropRows(row.projectId)
        throw cause
      }
      for (const row of written) patchCachedRows(row.projectId, new Map([[row.id, row]]))
    },

    saveSelectionResults: async (projectId, entries) => {
      if (!entries.length) return
      const written = new Map<string, StoredPhoto>()
      try {
        await withStores([STORE_PHOTOS], 'readwrite', async transaction => {
          // 読みを先に全部出し、書きも待たずに出す（1 件ずつ往復しない。W12）。
          // 同じ id が 2 回あっても、書くのは星だけなので最後の値が残る（前と同じ）。
          const rows = await Promise.all(entries.map(entry => getOne<StoredPhoto>(transaction, STORE_PHOTOS, entry.id)))
          const writes: Promise<unknown>[] = []
          entries.forEach((entry, index) => {
            const row = rows[index]
            if (!row || row.projectId !== projectId) return
            const next = { ...row, rating: Math.min(MAX_RATING, Math.max(0, entry.rating)) }
            writes.push(putOne(transaction, STORE_PHOTOS, next))
            written.set(next.id, next)
          })
          await Promise.all(writes)
        })
      } catch (cause) {
        dropRows(projectId)
        throw cause
      }
      patchCachedRows(projectId, written)
    },

    resetRatings: async projectId => {
      try {
        await withStores([STORE_PHOTOS], 'readwrite', async transaction => {
          const rows = await photosOfProject(transaction, projectId)
          await Promise.all(rows
            .filter(row => row.rating !== 0)
            .map(row => putOne(transaction, STORE_PHOTOS, { ...row, rating: 0 })))
        })
      } finally {
        dropRows(projectId)
      }
    },

    moveRating: async (projectId, fromRating, toRating, include, exclude) => {
      try {
        return await withStores([STORE_PHOTOS], 'readwrite', async transaction => {
          const rows = await photosOfProject(transaction, projectId)
          const targets = rows.filter(row =>
            !row.isMissing && row.rating === fromRating && (include ? include.has(row.id) : !exclude.has(row.id)))
          await Promise.all(targets.map(row => putOne(transaction, STORE_PHOTOS, { ...row, rating: toRating })))
          return targets.length
        })
      } finally {
        dropRows(projectId)
      }
    },

    readSession: projectId =>
      withStores([STORE_STATES], 'readonly', async transaction => {
        const row = await getOne<{ projectId: string, stateJson: string }>(transaction, STORE_STATES, projectId)
        return row?.stateJson ?? null
      }),

    writeSession: async (projectId, stateJson) => {
      await withStores([STORE_STATES], 'readwrite', transaction =>
        putOne(transaction, STORE_STATES, { projectId, stateJson, updatedAt: Date.now() }))
    },

    readPairOverrides: async projectId => {
      const row = await withStores([STORE_BURST_SHAPES], 'readonly', transaction =>
        getOne<{ projectId: string, overrides: PairOverride[] }>(
          transaction, STORE_BURST_SHAPES, overridesKey(projectId)
        ))
      return Array.isArray(row?.overrides) ? row.overrides : []
    },

    writePairOverrides: async (projectId, overrides) => {
      await withStores([STORE_BURST_SHAPES], 'readwrite', transaction =>
        putOne(transaction, STORE_BURST_SHAPES, {
          projectId: overridesKey(projectId),
          overrides,
          updatedAt: Date.now()
        }))
    },

    readSidecarState: async projectId => {
      const row = await withStores([STORE_STATES], 'readonly', transaction =>
        getOne<{ seenAt?: number, seenBy?: string, localChanged?: boolean }>(
          transaction, STORE_STATES, sidecarStateKey(projectId)
        ))
      return {
        seenAt: typeof row?.seenAt === 'number' ? row.seenAt : 0,
        seenBy: typeof row?.seenBy === 'string' ? row.seenBy : '',
        localChanged: row?.localChanged === true
      }
    },

    writeSidecarState: async (projectId, state) => {
      await withStores([STORE_STATES], 'readwrite', transaction =>
        putOne(transaction, STORE_STATES, {
          projectId: sidecarStateKey(projectId),
          seenAt: state.seenAt,
          seenBy: state.seenBy,
          localChanged: state.localChanged,
          updatedAt: Date.now()
        }))
    },

    deviceId: () =>
      // 読むと作るを同じトランザクションに入れ、2 つのタブが別々の id を作らないようにする。
      withStores([STORE_STATES], 'readwrite', async transaction => {
        const row = await getOne<{ projectId: string, id?: string }>(transaction, STORE_STATES, DEVICE_KEY)
        if (row?.id) return row.id
        const id = randomUUID()
        await putOne(transaction, STORE_STATES, { projectId: DEVICE_KEY, id, updatedAt: Date.now() })
        return id
      }),

    readDisplayEdge: async () => {
      const row = await withStores([STORE_STATES], 'readonly', transaction =>
        getOne<{ projectId: string, edge?: number }>(transaction, STORE_STATES, DISPLAY_EDGE_KEY))
      return typeof row?.edge === 'number' ? row.edge : null
    },

    writeDisplayEdge: async edge => {
      await withStores([STORE_STATES], 'readwrite', transaction =>
        putOne(transaction, STORE_STATES, { projectId: DISPLAY_EDGE_KEY, edge, updatedAt: Date.now() }))
    },

    readAmazonLinks: async projectId => {
      const row = await withStores([STORE_STATES], 'readonly', transaction =>
        getOne<{ projectId: string, links: Record<string, string> }>(
          transaction, STORE_STATES, amazonLinksKey(projectId)
        ))
      return row?.links ?? null
    },

    writeAmazonLinks: async (projectId, links) => {
      await withStores([STORE_STATES], 'readwrite', transaction =>
        putOne(transaction, STORE_STATES, { projectId: amazonLinksKey(projectId), links, updatedAt: Date.now() }))
    },

    readHandle: async projectId => {
      const row = await withStores([STORE_STATES], 'readonly', transaction =>
        getOne<{ projectId: string, handle: FileSystemDirectoryHandle }>(
          transaction, STORE_STATES, handleKey(projectId)
        ))
      return row?.handle ?? null
    },

    writeHandle: async (projectId, handle) => {
      await withStores([STORE_STATES], 'readwrite', transaction =>
        putOne(transaction, STORE_STATES, { projectId: handleKey(projectId), handle, updatedAt: Date.now() }))
    }
  }
}
