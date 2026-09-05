/**
 * 端末内の保存領域。iPad ではここが「DB」になる。
 *
 * **IndexedDB を使う。** SQLite の wasm（wa-sqlite）+ OPFS も検討したが、
 * - 依存も wasm も増えず、iPadOS の OPFS まわりの不確実さを避けられる
 * - 絞り込み・並べ替えを純粋な関数（`utils/photoQuery.ts`）に置けるので
 *   **Node 上でテストできる**（SQL を wasm で動かすとテストできない）
 * ため、こちらを選んだ。行数は 1 プロジェクト 5,000 件程度で、
 * IndexedDB には十分小さい。
 *
 * サムネイルは**別ストア**に置く。一覧のために行を読むだけのとき、
 * 画像の実体まで引きずらないため。
 */
import type { Photo, SelectionSession } from '~/types/photo'
import type { TimestampSource } from '~/utils/captureTime'

const DATABASE_NAME = 'photo-curator'
// 2 で `burstShapes` を足した。`onupgradeneeded` は「無ければ作る」だけなので、
// 既存のストアと中身はそのまま残る。
const DATABASE_VERSION = 2

export const STORE_PROJECTS = 'projects'
export const STORE_PHOTOS = 'photos'
export const STORE_THUMBNAILS = 'thumbnails'
export const STORE_STATES = 'states'
export const STORE_BURST_SHAPES = 'burstShapes'

/** `photos` テーブルに相当する 1 行。デスクトップの列名に寄せてある。 */
export interface StoredPhoto {
  id: string
  projectId: string
  /** ブラウザでは原本を保存しないので、取り込み時のファイル名だけを残す。 */
  path: string
  relativePath: string
  name: string
  capturedAt: number | null
  timestampSource: TimestampSource
  dHash: string | null
  rating: number
  isMissing: boolean
  /** 解析できなかった理由。成功したら null に戻す。 */
  analysisError: string | null
}

export interface StoredProject {
  id: string
  name: string
  photoCount: number
  status: 'new' | 'scanning' | 'ready' | 'missing'
  createdAt: number
  updatedAt: number
  burstThreshold: number | null
  burstThresholdLearnedAt: number | null
}

let opening: Promise<IDBDatabase> | null = null

/**
 * DB を開く。**スキーマの追加は冪等**にしておき、既存データを壊さない
 * （デスクトップ側の migration と同じ方針）。
 */
export function openStore(): Promise<IDBDatabase> {
  if (opening) return opening
  opening = new Promise((resolve, reject) => {
    const request = indexedDB.open(DATABASE_NAME, DATABASE_VERSION)
    request.onupgradeneeded = () => {
      const db = request.result
      if (!db.objectStoreNames.contains(STORE_PROJECTS)) {
        db.createObjectStore(STORE_PROJECTS, { keyPath: 'id' })
      }
      if (!db.objectStoreNames.contains(STORE_PHOTOS)) {
        const photos = db.createObjectStore(STORE_PHOTOS, { keyPath: 'id' })
        photos.createIndex('projectId', 'projectId', { unique: false })
      }
      if (!db.objectStoreNames.contains(STORE_THUMBNAILS)) {
        db.createObjectStore(STORE_THUMBNAILS, { keyPath: 'photoId' })
      }
      if (!db.objectStoreNames.contains(STORE_STATES)) {
        db.createObjectStore(STORE_STATES, { keyPath: 'projectId' })
      }
      // 手で直したまとめの例外。1 プロジェクト 1 行に畳んで持つ。
      // 触ったペアの数だけしか増えないので、行に収めて構わない。
      if (!db.objectStoreNames.contains(STORE_BURST_SHAPES)) {
        db.createObjectStore(STORE_BURST_SHAPES, { keyPath: 'projectId' })
      }
    }
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(request.error ?? new Error('保存領域を開けませんでした。'))
  })
  return opening
}

const asPromise = <T>(request: IDBRequest<T>) =>
  new Promise<T>((resolve, reject) => {
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(request.error ?? new Error('保存領域の操作に失敗しました。'))
  })

/** 1 つのトランザクションを開いて渡す。完了まで待つ。 */
export async function withStores<T>(
  names: string[],
  mode: IDBTransactionMode,
  body: (transaction: IDBTransaction) => Promise<T> | T
): Promise<T> {
  const db = await openStore()
  const transaction = db.transaction(names, mode)
  const done = new Promise<void>((resolve, reject) => {
    transaction.oncomplete = () => resolve()
    transaction.onerror = () => reject(transaction.error ?? new Error('保存に失敗しました。'))
    transaction.onabort = () => reject(transaction.error ?? new Error('保存が中断されました。'))
  })
  const result = await body(transaction)
  await done
  return result
}

export const getAll = <T>(transaction: IDBTransaction, store: string) =>
  asPromise<T[]>(transaction.objectStore(store).getAll() as IDBRequest<T[]>)

export const getOne = <T>(transaction: IDBTransaction, store: string, key: IDBValidKey) =>
  asPromise<T | undefined>(transaction.objectStore(store).get(key) as IDBRequest<T | undefined>)

export const putOne = (transaction: IDBTransaction, store: string, value: unknown) =>
  asPromise(transaction.objectStore(store).put(value as never))

export const deleteOne = (transaction: IDBTransaction, store: string, key: IDBValidKey) =>
  asPromise(transaction.objectStore(store).delete(key))

/** あるプロジェクトの写真行をすべて読む。索引で引くので他プロジェクトは触らない。 */
export function photosOfProject(transaction: IDBTransaction, projectId: string) {
  const index = transaction.objectStore(STORE_PHOTOS).index('projectId')
  return asPromise<StoredPhoto[]>(index.getAll(projectId) as IDBRequest<StoredPhoto[]>)
}

/** 保存されたセッション JSON。無ければ null。 */
export async function readSession(projectId: string): Promise<string | null> {
  return withStores([STORE_STATES], 'readonly', async transaction => {
    const row = await getOne<{ projectId: string, stateJson: string }>(transaction, STORE_STATES, projectId)
    return row?.stateJson ?? null
  })
}

export async function writeSession(session: SelectionSession): Promise<void> {
  await withStores([STORE_STATES], 'readwrite', transaction =>
    putOne(transaction, STORE_STATES, {
      projectId: session.projectId,
      stateJson: JSON.stringify(session),
      updatedAt: Date.now()
    })
  )
}

/** 手で直したまとめの例外。キーは `pairKey`、値は繋ぐ(true)/切る(false)。 */
export async function readPairOverrides(projectId: string): Promise<Map<string, boolean>> {
  const row = await withStores([STORE_BURST_SHAPES], 'readonly', transaction =>
    getOne<{ projectId: string, overrides: Record<string, boolean> }>(
      transaction, STORE_BURST_SHAPES, projectId
    )
  )
  return new Map(Object.entries(row?.overrides ?? {}))
}

export async function writePairOverrides(
  projectId: string, overrides: Map<string, boolean>
): Promise<void> {
  await withStores([STORE_BURST_SHAPES], 'readwrite', transaction =>
    putOne(transaction, STORE_BURST_SHAPES, {
      projectId,
      overrides: Object.fromEntries(overrides),
      updatedAt: Date.now()
    })
  )
}

/**
 * 端末の保存領域を消えにくくする。iOS は容量が逼迫すると回収するので、
 * 取り込みの前に一度だけ頼んでおく。断られても取り込み自体は続けられる。
 */
export async function requestPersistence(): Promise<boolean> {
  try {
    if (!navigator.storage?.persist) return false
    if (await navigator.storage.persisted()) return true
    return await navigator.storage.persist()
  } catch {
    return false
  }
}

/** 保存済みの行を画面が使う `Photo` に直す。URL は呼び出し側が埋める。 */
export function toPhoto(row: StoredPhoto, thumbnailUrl: string | null, originalUrl: string | null): Photo {
  return {
    id: row.id,
    projectId: row.projectId,
    // 原本が手元に無ければサムネイルを見せる（拡大は 256px になる）。
    path: originalUrl ?? thumbnailUrl ?? '',
    relativePath: row.relativePath,
    name: row.name,
    capturedAt: row.capturedAt,
    dHash: row.dHash,
    rating: row.rating,
    thumbnailPath: thumbnailUrl
  }
}
