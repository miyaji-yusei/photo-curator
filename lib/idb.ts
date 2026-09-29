// ごく小さな IndexedDB のキー・値ストア。プロジェクトの一覧・設定・
// セッション・フォルダの handle を置く（Web フォルダ backend が使う）。
// ライブラリを増やさない（依存を最小限にするため）。

const DB_NAME = 'photo-curator'
const DB_VERSION = 1
const STORE = 'kv'

let dbPromise: Promise<IDBDatabase> | null = null

/**
 * 開けなかった理由を、次にすることが分かる文にする。
 *
 * 以前の版のデータが新しい版番号で残っていると（実ブラウザの localhost:3000 で
 * 実際にあった: 既存 v3 に対し v1 を要求）`VersionError` になる。IndexedDB は
 * 版を下げて開き直せないので、**データを勝手に消さず**、人に消してもらう。
 */
function describeOpenError(error: DOMException | null): Error {
  if (error?.name === 'VersionError') {
    return new Error(
      '以前の版のデータがこのブラウザに残っていて開けません。ブラウザの設定でこのサイトのデータ（IndexedDB）を消すと直ります。'
    )
  }
  return new Error(`端末の保存領域を開けませんでした（${error?.name ?? '不明'}）。`)
}

function openDb(): Promise<IDBDatabase> {
  if (dbPromise) return dbPromise
  const opening = new Promise<IDBDatabase>((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, DB_VERSION)
    request.onupgradeneeded = () => {
      const db = request.result
      if (!db.objectStoreNames.contains(STORE)) db.createObjectStore(STORE)
    }
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(describeOpenError(request.error))
    // 別のタブが古い版で開いたままだと、いつまでも待たされる。
    request.onblocked = () => reject(new Error('別のタブが保存領域を使っています。他の Photo Curator のタブを閉じてください。'))
  })
  dbPromise = opening
  // 失敗を覚えたままにしない。直したあとの次の呼び出しで開き直せるように。
  opening.catch(() => { if (dbPromise === opening) dbPromise = null })
  return opening
}

export async function idbGet<T>(key: string): Promise<T | null> {
  const db = await openDb()
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE, 'readonly')
    const request = tx.objectStore(STORE).get(key)
    request.onsuccess = () => resolve((request.result ?? null) as T | null)
    request.onerror = () => reject(request.error)
  })
}

export async function idbSet(key: string, value: unknown): Promise<void> {
  const db = await openDb()
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE, 'readwrite')
    tx.objectStore(STORE).put(value, key)
    tx.oncomplete = () => resolve()
    tx.onerror = () => reject(tx.error)
  })
}

export async function idbDelete(key: string): Promise<void> {
  const db = await openDb()
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE, 'readwrite')
    tx.objectStore(STORE).delete(key)
    tx.oncomplete = () => resolve()
    tx.onerror = () => reject(tx.error)
  })
}

/** 前方一致するキーを全部探す（`project:` の一覧などに使う）。 */
export async function idbKeysWithPrefix(prefix: string): Promise<string[]> {
  const db = await openDb()
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE, 'readonly')
    const request = tx.objectStore(STORE).getAllKeys()
    request.onsuccess = () => {
      const keys = (request.result as IDBValidKey[]).filter(
        (key): key is string => typeof key === 'string' && key.startsWith(prefix)
      )
      resolve(keys)
    }
    request.onerror = () => reject(request.error)
  })
}
