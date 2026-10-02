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
import type { Photo } from '~/types/photo'
import type { TimestampSource } from '~/utils/captureTime'

/**
 * **旧版の DB（名前 `photo-curator`）は開かない・消さない。** 新しい名前で作り直す。
 * 旧版の行は写真の鍵が uuid・Session が旧形で、そのままでは core が読めないため。
 */
export const DATABASE_NAME = 'photo-curator-mb'
const DATABASE_VERSION = 1

export const STORE_PROJECTS = 'projects'
export const STORE_PHOTOS = 'photos'
export const STORE_THUMBNAILS = 'thumbnails'
export const STORE_STATES = 'states'
export const STORE_BURST_SHAPES = 'burstShapes'
/** 選別画面に出す表示用画像。サムネイルとは別ストアにして、一覧が引きずらない。 */
export const STORE_DISPLAYS = 'displays'

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
  /** 表示用画像を作ったときの長辺。設定を変えたときの作り直し判定に使う。 */
  displayEdge?: number | null
}

/** 写真の出所。行に持つので、リロード後も同じ出所から読み直せる。 */
export type StoredSource
  = | { kind: 'picker' }
  // handle は `states` の `handle:<projectId>` に置く。
    | { kind: 'folder', folderName: string }
    | { kind: 'dev', root: string }
  // Amazon Photos の共有リンク。`key` は `"{host}|{shareId}"`、`url` は入力されたリンク。
  // tempLink は `states` の `amazonLinks:<projectId>` に置く。
    | { kind: 'amazon', key: string, url: string }

export interface StoredProject {
  id: string
  name: string
  /** 省略はピッカー（旧版の行）。 */
  source?: StoredSource
  photoCount: number
  status: 'new' | 'scanning' | 'ready' | 'missing'
  createdAt: number
  updatedAt: number
  burstThreshold: number | null
  burstThresholdLearnedAt: number | null
  /** このプロジェクトの表示用画像の長辺。無ければアプリの既定。 */
  displayEdge?: number | null
  /** 同名の JPEG と RAW を 1 枚の写真として扱う。無ければ true（既定。U46）。 */
  pairRawJpeg?: boolean
  /** `pairRawJpeg` を切り替えた時刻（ms）。無い・0 は「一度も切り替えていない」（U48）。 */
  pairRawJpegAt?: number
}

let opening: Promise<IDBDatabase> | null = null

/**
 * 開けなかった理由を、「何が起きたか」と「次にすること」が分かる文にする。
 * IndexedDB は版を下げて開き直せないので、データを勝手に消さず、人に消してもらう。
 */
export function describeOpenError(error: DOMException | null): Error {
  if (error?.name === 'VersionError') {
    return new Error(
      `この端末の保存領域（${DATABASE_NAME}）が、より新しい版のアプリで作られていて開けません。`
      + 'アプリを最新の版に更新してください。直らないときは、ブラウザの設定でこのサイトのデータ（IndexedDB）を消してください。'
    )
  }
  return new Error(
    `端末の保存領域を開けませんでした（${error?.name ?? '不明'}）。`
    + 'ブラウザを再読み込みしてください。プライベートブラウズでは使えないことがあるので、通常のウィンドウで開き直してください。'
  )
}

export const BLOCKED_MESSAGE
  = '別のタブが古い版の保存領域を使っていて開けません。ほかの Photo Curator のタブをすべて閉じてから、再読み込みしてください。'

/**
 * DB を開く。**スキーマの追加は冪等**にしておき、既存データを壊さない
 * （デスクトップ側の migration と同じ方針）。
 */
export function openStore(): Promise<IDBDatabase> {
  if (opening) return opening
  const attempt = new Promise<IDBDatabase>((resolve, reject) => {
    let settled = false
    const fail = (error: Error) => {
      settled = true
      reject(error)
    }
    let request: IDBOpenDBRequest
    try {
      request = indexedDB.open(DATABASE_NAME, DATABASE_VERSION)
    } catch (cause) {
      fail(describeOpenError(cause instanceof DOMException ? cause : null))
      return
    }
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
      // Session・フォルダの handle（`handle:<projectId>`）を置く。
      if (!db.objectStoreNames.contains(STORE_STATES)) {
        db.createObjectStore(STORE_STATES, { keyPath: 'projectId' })
      }
      // 手で直したまとめの例外（`overrides:<projectId>`）。1 プロジェクト 1 行に畳んで持つ。
      if (!db.objectStoreNames.contains(STORE_BURST_SHAPES)) {
        db.createObjectStore(STORE_BURST_SHAPES, { keyPath: 'projectId' })
      }
      if (!db.objectStoreNames.contains(STORE_DISPLAYS)) {
        db.createObjectStore(STORE_DISPLAYS, { keyPath: 'photoId' })
      }
    }
    request.onsuccess = () => {
      // すでに失敗として返した後に開けたものは閉じる（別のタブの版上げを塞がないため）。
      if (settled) request.result.close()
      else resolve(request.result)
    }
    request.onerror = () => fail(describeOpenError(request.error))
    // 別のタブが古い版で開いたままだと、いつまでも待たされる。
    request.onblocked = () => fail(new Error(BLOCKED_MESSAGE))
  })
  opening = attempt
  // 失敗を覚えたままにしない。直したあとの次の呼び出しで開き直せるように。
  attempt.catch(() => { if (opening === attempt) opening = null })
  return attempt
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
export function toPhoto(
  row: StoredPhoto,
  thumbnailUrl: string | null,
  originalUrl: string | null,
  displayUrl: string | null = null
): Photo {
  return {
    id: row.id,
    projectId: row.projectId,
    // 原本が手元に無ければ表示用、それも無ければサムネイル。
    // **表示用があるので、リロード後も選別の見えは落ちない。**
    path: originalUrl ?? displayUrl ?? thumbnailUrl ?? '',
    relativePath: row.relativePath,
    name: row.name,
    capturedAt: row.capturedAt,
    dHash: row.dHash,
    rating: row.rating,
    thumbnailPath: thumbnailUrl,
    displayPath: displayUrl ?? originalUrl ?? thumbnailUrl
  }
}
