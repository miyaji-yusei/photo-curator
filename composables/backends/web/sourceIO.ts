/**
 * 出所から原本のバイトを得る口（設計 10 章 §2 の SourceIO）。
 *
 * 実装は 3 つ。**どれも「読む」だけ**で、写真は書き換えない。
 * - `PickerIO`       … 写真ピッカーで選ばれた File を Map に持つ（iPad・フォルダ非対応のブラウザ）
 * - `HandleFolderIO` … File System Access の handle（Chrome・Edge の「フォルダを選ぶ」）
 * - `DevFolderIO`    … 開発用の HTTP（`server/api/dev-folder/`。`pnpm dev` のときだけ）
 *
 * 将来 Swift の殻に包むときは、`SourceIO` を実装した橋をここへ差し込む。
 */

export interface SourceEntry {
  name: string
  isDirectory: boolean
  size: number
  mtimeMs: number
}

export interface SourceIO {
  /** `subPath` は出所の根からの相対（根は空文字。区切りは `/`）。 */
  list(subPath: string): Promise<SourceEntry[]>
  readFile(subPath: string, name: string): Promise<File>
  /**
   * サイドカー（`.photo-curator/catalog.json`）。写真のフォルダに置くのはこれだけ。
   * `readwrite`＝読み書きできる／`readonly`＝読むだけ／`none`＝出所にフォルダが無い。
   * `readwrite` は許可を求めずに確かめる（利用者の操作が無くても呼べる）。
   */
  sidecarAccess(): Promise<SidecarAccessKind>
  readSidecar(): Promise<string | null>
  /** 原子的に書く。`fileName` は `catalog.json` か退避の `catalog.<id>.json`。 */
  writeSidecar(json: string, fileName: string): Promise<void>
}

export type SidecarAccessKind = 'readwrite' | 'readonly' | 'none'

export const SIDECAR_DIR = '.photo-curator'
export const SIDECAR_FILE = 'catalog.json'
/** `catalog.json` か `catalog.<英数字とハイフン>.json` だけ。別の名前・場所へ書かせない。 */
export const isSidecarFileName = (name: string) =>
  name === SIDECAR_FILE || /^catalog\.[A-Za-z0-9-]{1,64}\.json$/.test(name)

export const joinPath = (base: string, name: string) => (base ? `${base}/${name}` : name)

// ---------------------------------------------------------------------------
// ピッカー
// ---------------------------------------------------------------------------

/**
 * 写真ピッカーで選ばれたファイルを持つ。**保存はしない**（iOS には永続的な
 * ファイルハンドルが無く、リロードで参照が切れる）。
 * 鍵は写真の `relativePath`（同名は `utils/uniquePath.ts` で一意にしてある）。
 */
export class PickerIO implements SourceIO {
  private readonly files = new Map<string, File>()

  add(relativePath: string, file: File) {
    this.files.set(relativePath, file)
  }

  /** 選ばれたファイルの一覧（根の直下に平らに並べる。鍵に `/` を含んでも 1 つの名前として扱う）。 */
  list(subPath: string): Promise<SourceEntry[]> {
    if (subPath) return Promise.resolve([])
    return Promise.resolve([...this.files].map(([name, file]) => ({
      name, isDirectory: false, size: file.size, mtimeMs: file.lastModified
    })))
  }

  readFile(subPath: string, name: string): Promise<File> {
    const key = joinPath(subPath, name)
    const file = this.files.get(key)
    return file ? Promise.resolve(file) : Promise.reject(new Error('この写真の原本は、リロードで失われました。'))
  }

  has(relativePath: string) {
    return this.files.has(relativePath)
  }

  fileFor(relativePath: string): File | null {
    return this.files.get(relativePath) ?? null
  }

  delete(relativePath: string) {
    this.files.delete(relativePath)
  }

  // ピッカーにはフォルダが無い。サイドカーは読めも書けもしない。
  sidecarAccess(): Promise<SidecarAccessKind> {
    return Promise.resolve('none')
  }

  readSidecar(): Promise<string | null> {
    return Promise.resolve(null)
  }

  writeSidecar(): Promise<void> {
    return Promise.reject(new Error('この出所にはサイドカーを書けません。'))
  }
}

// ---------------------------------------------------------------------------
// File System Access
// ---------------------------------------------------------------------------

type PermissionState = 'granted' | 'denied' | 'prompt'
interface PermissionHandle {
  queryPermission?: (descriptor: { mode: 'read' | 'readwrite' }) => Promise<PermissionState>
  requestPermission?: (descriptor: { mode: 'read' | 'readwrite' }) => Promise<PermissionState>
}

/** 許可があるか。ダイアログは出さない（利用者の操作が無くても呼べる）。 */
export async function hasHandlePermission(handle: FileSystemDirectoryHandle): Promise<boolean> {
  try {
    const query = (handle as unknown as PermissionHandle).queryPermission
    if (!query) return true
    return (await query.call(handle, { mode: 'readwrite' })) === 'granted'
  } catch {
    return false
  }
}

/** 許可を求める。**利用者の操作（ボタン）の中で呼ぶこと。** */
export async function requestHandlePermission(handle: FileSystemDirectoryHandle): Promise<boolean> {
  try {
    const request = (handle as unknown as PermissionHandle).requestPermission
    if (!request) return true
    return (await request.call(handle, { mode: 'readwrite' })) === 'granted'
  } catch {
    return false
  }
}

export class HandleFolderIO implements SourceIO {
  constructor(private readonly root: FileSystemDirectoryHandle) {}

  /** 辿ったディレクトリ handle（サブパス → handle）。読み込みのたびに根から辿り直さない。 */
  private readonly dirs = new Map<string, FileSystemDirectoryHandle>()

  private async resolveDir(subPath: string): Promise<FileSystemDirectoryHandle> {
    let dir = this.root
    let walked = ''
    for (const part of subPath.split('/').filter(Boolean)) {
      walked = walked ? `${walked}/${part}` : part
      const cached = this.dirs.get(walked)
      if (cached) {
        dir = cached
        continue
      }
      dir = await dir.getDirectoryHandle(part)
      this.dirs.set(walked, dir)
    }
    return dir
  }

  async list(subPath: string): Promise<SourceEntry[]> {
    const dir = await this.resolveDir(subPath)
    const result: SourceEntry[] = []
    // `entries()` は非同期イテレータ。型定義が薄い環境がある。
    const entries = (dir as unknown as {
      entries: () => AsyncIterable<[string, FileSystemHandle]>
    }).entries()
    for await (const [name, handle] of entries) {
      if (handle.kind === 'directory') {
        result.push({ name, isDirectory: true, size: 0, mtimeMs: 0 })
      } else {
        // size・mtimeMs は走査の側で使わない。getFile() は 1 枚ごとにブラウザとファイルシステムの
        // 往復になるので呼ばない（読み込み時の File.lastModified は本物なので、撮影時刻の手がかりは残る）。
        result.push({ name, isDirectory: false, size: 0, mtimeMs: 0 })
      }
    }
    return result
  }

  async readFile(subPath: string, name: string): Promise<File> {
    const dir = await this.resolveDir(subPath)
    const handle = await dir.getFileHandle(name)
    return handle.getFile()
  }

  /** 書き込みの許可があれば readwrite。許可は求めない（求めるのはプロジェクトの画面のボタン）。 */
  async sidecarAccess(): Promise<SidecarAccessKind> {
    return (await hasHandlePermission(this.root)) ? 'readwrite' : 'readonly'
  }

  async readSidecar(): Promise<string | null> {
    try {
      const dir = await this.root.getDirectoryHandle(SIDECAR_DIR)
      const file = await (await dir.getFileHandle(SIDECAR_FILE)).getFile()
      return await file.text()
    } catch (cause) {
      if (cause instanceof DOMException && cause.name === 'NotFoundError') return null
      throw cause
    }
  }

  /**
   * 書く。`createWritable` は `.crswap` に書いて close で置き換える（ブラウザ側で原子的）。
   * **写真のフォルダに作るのは `.photo-curator/` だけ。**
   */
  async writeSidecar(json: string, fileName: string): Promise<void> {
    if (!isSidecarFileName(fileName)) throw new Error('サイドカーのファイル名が正しくありません。')
    const dir = await this.root.getDirectoryHandle(SIDECAR_DIR, { create: true })
    const handle = await dir.getFileHandle(fileName, { create: true })
    const writable = await handle.createWritable()
    try {
      await writable.write(json)
      await writable.close()
    } catch (cause) {
      await writable.abort().catch(() => undefined)
      throw cause
    }
  }
}

// ---------------------------------------------------------------------------
// 開発用 HTTP
// ---------------------------------------------------------------------------

/** 開発用。`server/api/dev-folder/` を叩くだけ。`process.dev` のときしか答えない。 */
export class DevFolderIO implements SourceIO {
  /** `list` で見えた更新時刻。`readFile` で File の `lastModified` に入れる（撮影時刻の手がかり）。 */
  private readonly mtimes = new Map<string, number>()

  constructor(private readonly root: string) {}

  async list(subPath: string): Promise<SourceEntry[]> {
    const url = `/api/dev-folder/list?root=${encodeURIComponent(this.root)}&path=${encodeURIComponent(subPath)}`
    const response = await fetch(url)
    if (!response.ok) throw new Error(`フォルダの一覧を読めませんでした（${response.status}）。パスを確かめてください。`)
    const entries = await response.json() as SourceEntry[]
    for (const entry of entries) this.mtimes.set(joinPath(subPath, entry.name), entry.mtimeMs)
    return entries
  }

  async readFile(subPath: string, name: string): Promise<File> {
    const full = joinPath(subPath, name)
    const url = `/api/dev-folder/file?root=${encodeURIComponent(this.root)}&path=${encodeURIComponent(full)}`
    const response = await fetch(url)
    if (!response.ok) throw new Error(`読めませんでした（${response.status}）`)
    const blob = await response.blob()
    return new File([blob], name, { lastModified: this.mtimes.get(full) ?? Date.now(), type: blob.type })
  }

  // 開発用の配信は読むだけ。サイドカーも読めるが書けない。
  sidecarAccess(): Promise<SidecarAccessKind> {
    return Promise.resolve('readonly')
  }

  async readSidecar(): Promise<string | null> {
    const path = `${SIDECAR_DIR}/${SIDECAR_FILE}`
    const url = `/api/dev-folder/file?root=${encodeURIComponent(this.root)}&path=${encodeURIComponent(path)}`
    const response = await fetch(url)
    if (response.status === 404) return null
    if (!response.ok) throw new Error(`サイドカーを読めませんでした（${response.status}）`)
    return response.text()
  }

  writeSidecar(): Promise<void> {
    return Promise.reject(new Error('開発用のフォルダにはサイドカーを書けません。'))
  }
}

// ---------------------------------------------------------------------------
// 組み合わせ
// ---------------------------------------------------------------------------

/** 出所ごとの `SourceIO` を作る窓口。Swift の殻では、これごと差し替える。 */
export interface SourceIOSet {
  picker: PickerIO
  forHandle: (handle: FileSystemDirectoryHandle) => SourceIO
  forDev: (root: string) => SourceIO
}

export function createSourceIOSet(): SourceIOSet {
  return {
    picker: new PickerIO(),
    forHandle: handle => new HandleFolderIO(handle),
    forDev: root => new DevFolderIO(root)
  }
}
