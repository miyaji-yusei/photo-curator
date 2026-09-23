// ごく小さな IndexedDB のキー・値ストア。プロジェクトの一覧・設定・
// セッション・フォルダの handle を置く（Web フォルダ backend が使う）。
// ライブラリを増やさない（依存を最小限にするため）。

const DB_NAME = 'photo-curator'
const DB_VERSION = 1
const STORE = 'kv'

let dbPromise: Promise<IDBDatabase> | null = null

function openDb(): Promise<IDBDatabase> {
  if (dbPromise) return dbPromise
  dbPromise = new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, DB_VERSION)
    request.onupgradeneeded = () => {
      const db = request.result
      if (!db.objectStoreNames.contains(STORE)) db.createObjectStore(STORE)
    }
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(request.error)
  })
  return dbPromise
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
