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
  /** サイドカー（T8）。書ける出所だけが持つ。 */
  writeSidecar?(json: string): Promise<void>
  readSidecar?(): Promise<string | null>
}

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

  private async resolveDir(subPath: string): Promise<FileSystemDirectoryHandle> {
    let dir = this.root
    for (const part of subPath.split('/').filter(Boolean)) {
      dir = await dir.getDirectoryHandle(part)
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
        const file = await (handle as FileSystemFileHandle).getFile()
        result.push({ name, isDirectory: false, size: file.size, mtimeMs: file.lastModified })
      }
    }
    return result
  }

  async readFile(subPath: string, name: string): Promise<File> {
    const dir = await this.resolveDir(subPath)
    const handle = await dir.getFileHandle(name)
    return handle.getFile()
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
