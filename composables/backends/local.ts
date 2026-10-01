/**
 * ブラウザ（PC のブラウザ・iPad など）向けの実装。端末の中だけで完結する。
 *
 * **4 つの部品を組み合わせるだけ**（設計 10 章 §2）。将来 Swift の殻に包むときは、
 * `blobStore`・`sourceIO`・`fetcher` を差し替えて `createLocalBackend` に渡す。
 * - `store`     … プロジェクト・写真の行・Session・手直し（IndexedDB `photo-curator-mb`）
 * - `blobStore` … サムネイル・表示用画像の実体と object URL
 * - `sourceIO`  … 出所から原本のバイトを得る（ピッカー・フォルダ・開発用 HTTP）
 * - `fetcher`   … Amazon のバイト（指紋・ZIP 用。既定は中継サーバー。表示は素の `<img>` で足りる）
 *
 * - 写真の出所は 3 通り。ピッカーは**原本を保存しない**（iOS には永続的なファイルハンドルが無く、
 *   リロードすると参照が切れる）。フォルダは handle を保存して、次に開いたとき読み直す。
 *   どちらも 256px のサムネイルと表示用画像・星を残すので、リロード後も選別を続けられる。
 * - 解析（撮影日時・サムネイル・指紋）はブラウザの復号器で行う。Safari は HEIC も読める。
 * - **写真ライブラリは書き換えない。** 反映は共有シートや ZIP 書き出しなど、利用者の操作を経由する。
 */
import type {
  AmazonExport, AmazonPreview, ExportReport, Photo, PhotoPage, PhotoSort, Project,
  ProjectProgress, ProjectTask, SelectionResult, SelectionSummary
} from '~/types/photo'
import { init as initCore } from '~/lib/core'
import { parseSavedSelection, serializeSavedSelection } from '~/utils/selectionFlow'
import type { SavedSelection } from '~/utils/selectionFlow'
import { MAX_RATING } from '~/types/photo'
import type { DeviceIdentity, PhotoBackend, SidecarAccess, SidecarState } from '~/composables/photoBackend'
import { analyzeAll, workersFor } from '~/utils/analysisPool'
import { fetchPool } from '~/utils/amazonPool'
import { keyFor, parseContentDate, parseKey, parseShareUrl, readShare, viewBoxUrl } from '~/lib/amazonShare'
import { resolveCaptureTime } from '~/utils/captureTime'
import { createStoredZip } from '~/utils/zip'
import { downloadBlob, zipEntriesByRating } from '~/utils/shareExport'
import type { AnalysisJob } from '~/utils/analysisPool'
import { DISPLAY_EDGE_DEFAULT, hashThumbnail } from '~/utils/analyzePhoto'
import { DISPLAY_EDGE_CHOICES, nearestDisplayEdge } from '~/utils/displayEdge'
import { capabilitiesFor, hasDirectoryPicker } from '~/utils/capabilities'
import { requestPersistence, toPhoto } from '~/utils/browserStore'
import type { StoredPhoto, StoredProject, StoredSource } from '~/utils/browserStore'
import { scanFolder } from '~/utils/folderScan'
import {
  comparePhotos, filterPhotos, pagePhotos, summarizeRatings
} from '~/utils/photoQuery'
import { orderForAnalysis } from '~/utils/analysisOrder'
import { uniquePaths } from '~/utils/uniquePath'
import { randomUUID } from '~/utils/uuid'
import type { Fetcher } from '~/composables/backends/web/fetcher'
import { relayFetcher } from '~/composables/backends/web/fetcher'
import type { BlobStore, PhotoUrls } from '~/composables/backends/web/blobStore'
import { createIdbBlobStore } from '~/composables/backends/web/blobStore'
import type { SourceIO, SourceIOSet } from '~/composables/backends/web/sourceIO'
import {
  createSourceIOSet, hasHandlePermission, requestHandlePermission
} from '~/composables/backends/web/sourceIO'
import type { WebStore } from '~/composables/backends/web/store'
import { createIdbStore } from '~/composables/backends/web/store'

/** 差し替えられる 4 つの部品。省略したものは IndexedDB などの既定を使う。 */
export interface LocalBackendParts {
  store: WebStore
  blobStore: BlobStore
  sourceIO: SourceIOSet
  fetcher: Fetcher
}

const PICKER_LABEL = 'この iPad の写真'
/** フォルダの読み直しに許可が要るときの案内。 */
const PERMISSION_MESSAGE
  = 'フォルダへのアクセスが許可されていません。プロジェクトの画面の「フォルダへのアクセスを許可」を押してください。'

const progressOf = (
  projectId: string, task: ProjectTask, phase: ProjectProgress['phase'],
  processed: number, total: number, message: string, failed = 0
): ProjectProgress => ({ projectId, task, phase, processed, total, message, warning: null, failed })

const sourceOf = (row: StoredProject): StoredSource => row.source ?? { kind: 'picker' }

/** `a/b/c.jpg` の `c.jpg` を除いた `a/b`。根の直下なら空。 */
function subPathOf(row: StoredPhoto): string {
  const cut = row.relativePath.length - row.name.length - 1
  return cut > 0 && row.relativePath.endsWith(`/${row.name}`) ? row.relativePath.slice(0, cut) : ''
}

/** 共有が消えている（404）ときの理由。`lib/amazonShare.ts` の文と同じ。 */
const LINK_GONE = 'このリンクは削除されたか、無効です。'
/** サムネイルの長辺（08章 6「絵の 3 段」）。指紋もここから作る（PC と同じ）。 */
const AMAZON_THUMB_EDGE = 160
const SAMPLE_LIMIT = 12
/** 指紋作りの並列。Amazon への通信は控えめに。 */
const HASH_CONCURRENCY = 4
/** 何枚ごとに行へ書くか。 */
const HASH_SAVE_EVERY = 20
const NO_RELAY_WARNING = '中継サーバーが無いため、連写は自動ではまとまりません（表示だけで選別できます）。'
const ONLY_CSV = '原本を取れませんでした。CSV だけ書き出せます。'

const unsupported = (what: string) =>
  Promise.reject(new Error(`${what}はこの端末では行えません。ブラウザから写真ライブラリを書き換えられないためです。`))

export function createLocalBackend(parts: Partial<LocalBackendParts> = {}): PhotoBackend {
  const store = parts.store ?? createIdbStore()
  const blobStore = parts.blobStore ?? createIdbBlobStore()
  const sourceIO = parts.sourceIO ?? createSourceIOSet()
  // Amazon の指紋・ZIP が使う。既定は中継（Nitro）。無ければ null が返るだけ。
  const fetcher = parts.fetcher ?? relayFetcher

  const capabilities = capabilitiesFor('browser', { directoryPicker: hasDirectoryPicker() })
  const workers = workersFor(hasDirectoryPicker())

  /** 原本はセッション中だけ持つ（ピッカー）。リロードで消えるが、保存もしない。 */
  const originals = new Map<string, File>()
  const listeners = new Set<(progress: ProjectProgress) => void>()
  const cancelled = new Set<string>()
  /** 準備が走っているプロジェクト。同じものを二重に走らせない。 */
  const preparing = new Set<string>()
  /** Amazon の tempLink（node id → URL）。Amazon でなければ null。読んだ結果を覚える。 */
  const linkCache = new Map<string, Record<string, string> | null>()
  /** 中継が無いと分かったプロジェクト。**このページを開いている間**は、準備・続きで叩き直さない。 */
  const noRelay = new Set<string>()
  /** 「フォルダを選ぶ」で選ばれた、まだプロジェクトにならない handle。 */
  let pendingFolder: FileSystemDirectoryHandle | null = null

  const emit = (progress: ProjectProgress) => {
    for (const listener of listeners) listener(progress)
  }

  async function asProject(row: StoredProject): Promise<Project> {
    const source = sourceOf(row)
    let folderPath = PICKER_LABEL
    let folderAccess: Project['folderAccess']
    if (source.kind === 'folder') {
      folderPath = source.folderName
      const handle = await store.readHandle(row.id).catch(() => null)
      folderAccess = handle && await hasHandlePermission(handle) ? 'granted' : 'needs-permission'
    } else if (source.kind === 'dev') {
      folderPath = source.root
    } else if (source.kind === 'amazon') {
      folderPath = source.url
    }
    return {
      id: row.id,
      name: row.name,
      folderPath,
      photoCount: row.photoCount,
      status: row.status,
      createdAt: row.createdAt,
      updatedAt: row.updatedAt,
      burstThreshold: row.burstThreshold,
      burstThresholdLearnedAt: row.burstThresholdLearnedAt,
      sourceKind: source.kind === 'amazon' ? 'amazon' : 'folder',
      source: source.kind,
      ...(folderAccess ? { folderAccess } : {})
    }
  }

  /**
   * 出所の `SourceIO`。フォルダは許可が要る。`request` が true のときだけ許可を求める
   * （**利用者の操作の中でだけ**）。得られなければ null。
   */
  async function ioFor(row: StoredProject, request: boolean): Promise<SourceIO | null> {
    const source = sourceOf(row)
    if (source.kind === 'dev') return sourceIO.forDev(source.root)
    if (source.kind === 'picker') return sourceIO.picker
    // Amazon は出所からファイルを読まない（URL で出し、バイトは fetcher で取る）。
    if (source.kind === 'amazon') return null
    const handle = await store.readHandle(row.id)
    if (!handle) return null
    if (!(await hasHandlePermission(handle))) {
      if (!request || !(await requestHandlePermission(handle))) return null
    }
    return sourceIO.forHandle(handle)
  }

  async function linksOf(projectId: string): Promise<Record<string, string> | null> {
    if (linkCache.has(projectId)) return linkCache.get(projectId) ?? null
    const links = await store.readAmazonLinks(projectId).catch(() => null)
    linkCache.set(projectId, links)
    return links
  }

  /**
   * Amazon の写真に URL を付ける。**画像は端末に置かず、URL をそのまま `<img>` に渡す**
   * （crossOrigin を付けない素の `<img>` なら、CORS の無い画像 CDN からも出せる）。
   * サムネイルは指紋を作るときに取れていればその実体、無ければ `viewBox=160`。
   * 表示用は `viewBox=<長辺>`、原本（`path`）は tempLink そのもの。
   */
  async function decorateAmazon(rows: StoredPhoto[], links: Record<string, string>): Promise<Photo[]> {
    const urls = await blobStore.load(rows.map(row => row.id))
    // 表示用の URL の長辺は、そのプロジェクトで選んだ値（無ければアプリの既定）。
    const edge = rows.length ? await displayEdgeOf(rows[0]!.projectId) : DISPLAY_EDGE_DEFAULT
    return rows.map(row => {
      const found = urls.get(row.id)
      const link = links[row.relativePath] ?? null
      return toPhoto(
        row,
        found?.thumbnailUrl ?? (link ? viewBoxUrl(link, AMAZON_THUMB_EDGE) : null),
        link,
        found?.displayUrl ?? (link ? viewBoxUrl(link, edge) : null)
      )
    })
  }

  /** 行に URL を付けて画面が使える形にする。サムネイルは 1 枚ずつ読む。 */
  async function decorate(
    rows: StoredPhoto[], projectId?: string, options?: { display?: boolean }
  ): Promise<Photo[]> {
    const links = rows.length ? await linksOf(rows[0]!.projectId) : null
    if (links) return decorateAmazon(rows, links)
    const urls = await blobStore.load(rows.map(row => row.id), options)
    const fallbacks = projectId ? await originalsOfUnprepared(rows, urls, projectId) : new Map<string, string>()
    return rows.map(row => {
      const found: PhotoUrls | undefined = urls.get(row.id)
      const file = originals.get(row.id)
      const originalUrl = file ? blobStore.originalUrl(row.id, file) : fallbacks.get(row.id) ?? null
      return toPhoto(row, found?.thumbnailUrl ?? null, originalUrl, found?.displayUrl ?? null)
    })
  }

  /**
   * まだ準備が済んでいない写真（サムネイルも表示用も無い）だけ、出所から原本を読んで URL にする。
   * 準備の途中で「選別を開始」に進んだとき、絵が空にならないように。**許可は求めない**し、
   * 1 回に読むのは少数だけ（一覧を引くたびに原本を読み込まない）。
   */
  async function originalsOfUnprepared(
    rows: StoredPhoto[], urls: Map<string, PhotoUrls>, projectId: string
  ): Promise<Map<string, string>> {
    const result = new Map<string, string>()
    const pending = rows.filter(row => {
      const found = urls.get(row.id)
      return !found?.thumbnailUrl && !found?.displayUrl && !originals.has(row.id) && !row.isMissing
    }).slice(0, 12)
    if (!pending.length) return result
    const project = await store.getProject(projectId)
    if (!project || sourceOf(project).kind === 'picker') return result
    const io = await ioFor(project, false).catch(() => null)
    if (!io) return result
    for (const row of pending) {
      try {
        const file = await io.readFile(subPathOf(row), row.name)
        result.set(row.id, blobStore.originalUrl(row.id, file))
      } catch {
        // 読めなければ空のまま。準備が進めばサムネイルに落ちる。
      }
    }
    return result
  }

  /**
   * サイドカーを扱う `SourceIO`。**許可は求めない**（開くたびに呼ぶので、ダイアログを出さない）。
   * フォルダの handle が無い・ピッカーは、書けない出所として扱う。
   */
  async function sidecarIO(projectId: string): Promise<SourceIO | null> {
    const row = await store.getProject(projectId)
    if (!row) return null
    const source = sourceOf(row)
    if (source.kind === 'dev') return sourceIO.forDev(source.root)
    if (source.kind === 'picker') return sourceIO.picker
    // Amazon にサイドカーは無い（none）。結果は「この端末だけの結果」。
    if (source.kind === 'amazon') return null
    const handle = await store.readHandle(projectId).catch(() => null)
    return handle ? sourceIO.forHandle(handle) : null
  }

  const newRow = (projectId: string, id: string, relativePath: string, name: string): StoredPhoto => ({
    id,
    projectId,
    path: relativePath,
    relativePath,
    name,
    capturedAt: null,
    timestampSource: 'unknown',
    dHash: null,
    rating: 0,
    isMissing: false,
    analysisError: null
  })

  /**
   * 解析を並列に回す。**1 枚終わるごとに、その写真の行だけ**を書く（配列を書き直さない）。
   * 途中で閉じてもそこまでは残り、次は指紋の無い写真だけが対象になる。
   */
  /** このプロジェクトで表示用画像を作る長辺。プロジェクトの指定 → アプリの既定 → 既定値。 */
  async function displayEdgeOf(projectId: string): Promise<number> {
    const own = (await store.getProject(projectId))?.displayEdge
    const edge = own ?? await store.readDisplayEdge().catch(() => null)
    return edge ? nearestDisplayEdge(edge) : DISPLAY_EDGE_DEFAULT
  }

  async function analyze(projectId: string, jobs: AnalysisJob[]): Promise<void> {
    const total = jobs.length
    const displayEdge = await displayEdgeOf(projectId)
    let processed = 0
    let failed = 0
    emit(progressOf(projectId, 'background', 'hashing', 0, total, '写真を解析しています'))
    await analyzeAll(jobs, {
      workers,
      isCancelled: () => cancelled.has(projectId),
      displayEdge,
      onResult: async (id, analyzed) => {
        processed += 1
        if (analyzed.error) failed += 1
        // 画像を先に書く。行に指紋があるなら、画像もあるようにするため。
        await blobStore.put(id, { thumbnail: analyzed.thumbnail, display: analyzed.display })
        await store.patchPhoto(id, {
          capturedAt: analyzed.capturedAt,
          timestampSource: analyzed.timestampSource,
          dHash: analyzed.dHash,
          analysisError: analyzed.error,
          displayEdge: analyzed.display ? displayEdge : null
        })
        emit(progressOf(projectId, 'background', 'hashing', processed, total, '写真を解析しています', failed))
      }
    })
    const phase = cancelled.has(projectId) ? 'cancelled' : 'complete'
    emit(progressOf(projectId, 'background', phase, processed, total, '解析が終わりました', failed))
    await store.patchProject(projectId, {})
  }

  /** 指紋がまだ無い（かつ失敗もしていない）写真の解析を、出所から読んで回す。 */
  async function analyzeBacklog(projectId: string, io: SourceIO): Promise<void> {
    const rows = await store.photosOfProject(projectId)
    // 撮影時刻の昇順（選別の順）。分からないものは最後に取り込み順（`orderForAnalysis`）。
    const jobs: AnalysisJob[] = orderForAnalysis(
      rows.filter(row => !row.isMissing && row.dHash === null && row.analysisError === null),
      row => row
    ).map(row => ({ id: row.id, load: () => io.readFile(subPathOf(row), row.name) }))
    if (!jobs.length) return
    await analyze(projectId, jobs)
  }

  /**
   * フォルダの準備: 走査（行を作る）→ 解析（サムネイル・表示用・指紋）。
   * **走査が済んだ時点で `scan` を complete にする**ので、解析の途中でも選別を始められる。
   * 画面を移っても止まらない（走っているのは画面ではなくこのバックエンド）。
   */
  async function prepareFolder(projectId: string): Promise<void> {
    if (preparing.has(projectId)) {
      // すでに走っている。待っている画面（読み込み中のダイアログ）だけ閉じさせる。
      emit(progressOf(projectId, 'scan', 'complete', 0, 0, '準備は進んでいます'))
      return
    }
    preparing.add(projectId)
    cancelled.delete(projectId)
    let scanned = false
    try {
      const row = await store.getProject(projectId)
      if (!row) return
      emit(progressOf(projectId, 'scan', 'indexing', 0, 0, '写真フォルダを確認しています'))
      await store.patchProject(projectId, { status: 'scanning' })
      const io = await ioFor(row, true)
      if (!io) throw new Error(PERMISSION_MESSAGE)
      await requestPersistence()

      const files = await scanFolder(io, {
        isCancelled: () => cancelled.has(projectId),
        onFound: count => emit(progressOf(projectId, 'scan', 'indexing', count, 0, '写真フォルダを確認しています'))
      })
      if (cancelled.has(projectId)) {
        await store.patchProject(projectId, { status: row.photoCount ? 'ready' : 'new' })
        emit(progressOf(projectId, 'scan', 'cancelled', 0, 0, '読み込みを中断しました'))
        return
      }

      // 再走査でも同じ写真の行を使い回す（鍵は出所の中の相対パス）。星と指紋を失わない。
      const existing = await store.photosOfProject(projectId)
      const byPath = new Map(existing.map(item => [item.relativePath, item]))
      const seen = new Set<string>()
      const changed: StoredPhoto[] = []
      for (const file of files) {
        seen.add(file.relativePath)
        const found = byPath.get(file.relativePath)
        if (!found) changed.push(newRow(projectId, randomUUID(), file.relativePath, file.name))
        else if (found.isMissing) changed.push({ ...found, isMissing: false })
      }
      for (const item of existing) {
        if (!seen.has(item.relativePath) && !item.isMissing) changed.push({ ...item, isMissing: true })
      }
      await store.putPhotos(changed)
      await store.patchProject(projectId, { photoCount: files.length, status: 'ready' })
      scanned = true
      emit(progressOf(projectId, 'scan', 'complete', files.length, files.length, '読み込みが終わりました'))

      await analyzeBacklog(projectId, io)
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : 'フォルダを読み込めませんでした。'
      if (!scanned) {
        const current = await store.getProject(projectId).catch(() => undefined)
        await store.patchProject(projectId, { status: current?.photoCount ? 'ready' : 'new' }).catch(() => undefined)
      }
      emit(progressOf(projectId, scanned ? 'background' : 'scan', 'error', 0, 0, message))
    } finally {
      preparing.delete(projectId)
    }
  }

  // ---- Amazon Photos の共有リンク --------------------------------------

  const amazonSourceOf = (row: StoredProject) => {
    const source = sourceOf(row)
    return source.kind === 'amazon' ? source : null
  }

  /**
   * 準備: 一覧を読んで写真の行を作る（表示用・サムネイルは「URL で出せる」ので、これで準備済み）。
   * そのあと裏で、中継からサムネイルを取って指紋を作る。**失敗したら自動でやり直し続けない**
   * （リンクが消えていたら理由を出して `missing` にし、開くたびには読まない。利用者の「再試行」だけ）。
   */
  async function prepareAmazon(projectId: string): Promise<void> {
    if (preparing.has(projectId)) {
      emit(progressOf(projectId, 'scan', 'complete', 0, 0, '準備は進んでいます'))
      return
    }
    preparing.add(projectId)
    cancelled.delete(projectId)
    noRelay.delete(projectId)
    let scanned = false
    try {
      const row = await store.getProject(projectId)
      const source = row && amazonSourceOf(row)
      if (!row || !source) return
      const share = parseKey(source.key)
      if (!share) throw new Error('リンクの形が正しくありません。')
      emit(progressOf(projectId, 'scan', 'indexing', 0, 0, '共有リンクを読んでいます'))
      await store.patchProject(projectId, { status: 'scanning' })
      await requestPersistence()

      let read
      try {
        read = await readShare(share)
      } catch (cause) {
        const message = cause instanceof Error ? cause.message : 'Amazon から読めませんでした。'
        const status = message === LINK_GONE ? 'missing' : row.photoCount ? 'ready' : 'new'
        await store.patchProject(projectId, { status })
        emit(progressOf(projectId, 'scan', 'error', 0, 0, message))
        return
      }
      if (cancelled.has(projectId)) {
        await store.patchProject(projectId, { status: row.photoCount ? 'ready' : 'new' })
        emit(progressOf(projectId, 'scan', 'cancelled', 0, 0, '読み込みを中断しました'))
        return
      }

      // 鍵は node id。再読み込みでも同じ行を使い回し、星と指紋を失わない。
      const links: Record<string, string> = {}
      const existing = await store.photosOfProject(projectId)
      const byPath = new Map(existing.map(item => [item.relativePath, item]))
      const seen = new Set<string>()
      const changed: StoredPhoto[] = []
      for (const node of read.photos) {
        if (!node.tempLink) continue
        links[node.id] = node.tempLink
        seen.add(node.id)
        const at = parseContentDate(node.contentProperties?.contentDate)
        const capture = at !== null
          ? { at, source: 'exif_original' as const }
          : resolveCaptureTime(null, node.name, null)
        const found = byPath.get(node.id)
        const fields = {
          name: node.name, capturedAt: capture?.at ?? null, timestampSource: capture?.source ?? 'unknown' as const
        }
        if (!found) changed.push({ ...newRow(projectId, randomUUID(), node.id, node.name), ...fields })
        else if (found.isMissing || found.name !== node.name || found.capturedAt !== fields.capturedAt) {
          changed.push({ ...found, ...fields, isMissing: false })
        }
      }
      for (const item of existing) {
        if (!seen.has(item.relativePath) && !item.isMissing) changed.push({ ...item, isMissing: true })
      }
      await store.writeAmazonLinks(projectId, links)
      linkCache.set(projectId, links)
      await store.putPhotos(changed)
      await store.patchProject(projectId, { photoCount: seen.size, status: 'ready' })
      scanned = true
      emit(progressOf(projectId, 'scan', 'complete', seen.size, seen.size, '読み込みが終わりました'))

      await hashBacklog(projectId)
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : '共有リンクを読み込めませんでした。'
      if (!scanned) {
        const current = await store.getProject(projectId).catch(() => undefined)
        await store.patchProject(projectId, { status: current?.photoCount ? 'ready' : 'new' }).catch(() => undefined)
      }
      emit(progressOf(projectId, scanned ? 'background' : 'scan', 'error', 0, 0, message))
    } finally {
      preparing.delete(projectId)
    }
  }

  /**
   * 指紋のまだ無い写真のサムネイル（`viewBox=160`）を中継から取り、指紋を作る。
   * 並列 4、20 枚ごとに行へ書く。**最初の 4 枚が全部取れなければ、中継は無いとみなして打ち切る**
   * （指紋は無いまま。選別は始められる。連写が自動でまとまらないだけ）。
   * 星は行の別の欄なので、書くのは 1 行ずつの `patchPhoto`（星を巻き戻さない）。
   */
  async function hashBacklog(projectId: string): Promise<void> {
    const links = await linksOf(projectId)
    if (!links) return
    const rows = orderForAnalysis(
      (await store.photosOfProject(projectId))
        .filter(row => !row.isMissing && row.dHash === null && row.analysisError === null && links[row.relativePath]),
      row => row
    )
    if (!rows.length) return
    const total = rows.length
    let processed = 0
    let failed = 0
    emit(progressOf(projectId, 'background', 'hashing', 0, total, '写真の指紋を作っています'))
    let pending: { id: string, patch: Partial<StoredPhoto> }[] = []
    const flush = async () => {
      const batch = pending
      pending = []
      await Promise.all(batch.map(item => store.patchPhoto(item.id, item.patch)))
    }
    const report = await fetchPool({
      items: rows,
      concurrency: HASH_CONCURRENCY,
      giveUpAfter: HASH_CONCURRENCY,
      isCancelled: () => cancelled.has(projectId),
      fetch: row => fetcher.fetchBytes(links[row.relativePath]!, AMAZON_THUMB_EDGE),
      onResult: async (row, blob) => {
        processed += 1
        // 取れなかった行には何も書かない（次に準備したとき、中継があればやり直せる）。
        if (blob) {
          const hash = await hashThumbnail(blob).catch(() => null)
          if (hash) await blobStore.put(row.id, { thumbnail: blob })
          if (!hash) failed += 1
          pending.push({
            id: row.id,
            patch: { dHash: hash, analysisError: hash ? null : '指紋を作れませんでした。' }
          })
        } else {
          failed += 1
        }
        if (pending.length >= HASH_SAVE_EVERY) await flush()
        emit(progressOf(projectId, 'background', 'hashing', processed, total, '写真の指紋を作っています', failed))
      }
    })
    await flush()
    if (report.gaveUp) {
      noRelay.add(projectId)
      emit({
        // 取れなかったのは中継が無いため。写真ごとの失敗としては数えない。
        ...progressOf(projectId, 'background', 'complete', processed, total, '指紋は作りませんでした'),
        warning: NO_RELAY_WARNING
      })
    } else {
      const phase = cancelled.has(projectId) ? 'cancelled' : 'complete'
      emit(progressOf(projectId, 'background', phase, processed, total, '解析が終わりました', failed))
    }
    await store.patchProject(projectId, {})
  }

  /** Amazon の結果を ZIP にする。原本は中継で取る（CSV は全部の出所が `saveCsv`）。 */
  async function exportAmazon(projectId: string, photoIds: string[]): Promise<AmazonExport> {
    const links = await linksOf(projectId)
    if (!links) throw new Error('Amazon のプロジェクトではありません。')
    const wanted = new Set(photoIds)
    const rows = (await store.photosOfProject(projectId))
      .filter(row => !row.isMissing && wanted.has(row.id))
      .sort((left, right) => right.rating - left.rating || (left.relativePath < right.relativePath ? -1 : 1))
    if (!rows.length) throw new Error('対象の写真がありません。')
    const stamp = new Date().toISOString().slice(0, 10)
    const got = new Map<string, Blob>()
    await fetchPool({
      items: rows,
      concurrency: HASH_CONCURRENCY,
      giveUpAfter: HASH_CONCURRENCY,
      fetch: row => links[row.relativePath] ? fetcher.fetchBytes(links[row.relativePath]!) : Promise.resolve(null),
      onResult: (row, blob) => { if (blob) got.set(row.id, blob) }
    })
    if (!got.size) throw new Error(ONLY_CSV)
    const taken = rows.filter(row => got.has(row.id))
    const zip = await createStoredZip(zipEntriesByRating(taken.map(row => ({
      name: row.name, rating: row.rating, blob: got.get(row.id)!, modifiedAt: row.capturedAt ?? undefined
    }))))
    return { blob: zip, fileName: `photo-curator-${stamp}.zip`, count: taken.length, skipped: rows.length - taken.length }
  }

  return {
    kind: 'local',
    capabilities,

    /**
     * フォルダを選ぶ（File System Access）。選んだ handle は、プロジェクトを作るときに保存する。
     * 戻り値は画面に出すフォルダ名。取り消したら null。
     */
    chooseFolder: async () => {
      const picker = (window as unknown as {
        showDirectoryPicker?: (options?: { mode?: 'read' | 'readwrite' }) => Promise<FileSystemDirectoryHandle>
      }).showDirectoryPicker
      if (!picker) return null
      try {
        // 星をサイドカーに書ける（T8）ように、最初から書き込みの許可も頼む。
        pendingFolder = await picker.call(window, { mode: 'readwrite' })
        return pendingFolder.name
      } catch (cause) {
        if (cause instanceof DOMException && cause.name === 'AbortError') return null
        throw cause
      }
    },

    onProjectProgress: (callback) => {
      listeners.add(callback)
      return Promise.resolve(() => listeners.delete(callback))
    },

    listProjects: async () => {
      const rows = await store.listProjects()
      return Promise.all(rows.sort((left, right) => right.updatedAt - left.updatedAt).map(asProject))
    },

    /**
     * `folderPath` が `dev:<絶対パス>` なら開発用のフォルダ、選んだフォルダの名前なら
     * そのフォルダ（handle を保存する）、空ならピッカー。
     */
    createProject: async (name: string, folderPath: string) => {
      const now = Date.now()
      const id = randomUUID()
      let source: StoredSource = { kind: 'picker' }
      let handle: FileSystemDirectoryHandle | null = null
      if (folderPath.startsWith('dev:')) {
        source = { kind: 'dev', root: folderPath.slice('dev:'.length) }
      } else if (folderPath && pendingFolder) {
        handle = pendingFolder
        source = { kind: 'folder', folderName: handle.name }
      }
      const row: StoredProject = {
        id,
        name: name.trim() || '新しいプロジェクト',
        source,
        photoCount: 0,
        status: 'new',
        createdAt: now,
        updatedAt: now,
        burstThreshold: null,
        burstThresholdLearnedAt: null
      }
      await store.putProject(row)
      if (handle) {
        await store.writeHandle(id, handle)
        pendingFolder = null
      }
      return asProject(row)
    },

    amazonPreview: async (shareUrl: string): Promise<AmazonPreview> => {
      const source = parseShareUrl(shareUrl)
      if (!source) throw new Error('Amazon Photos の共有リンクの形ではありません。')
      const share = await readShare(source)
      // 見本は `viewBox=160` の URL をそのまま `<img>` に渡す（バイトは読まない）。
      const samples = share.photos
        .slice(0, SAMPLE_LIMIT)
        .flatMap(node => (node.tempLink ? [viewBoxUrl(node.tempLink, AMAZON_THUMB_EDGE)] : []))
      return { key: share.key, name: share.name, count: share.photos.length, samples }
    },

    createAmazonProject: async (name: string, shareUrl: string) => {
      const source = parseShareUrl(shareUrl)
      if (!source) throw new Error('Amazon Photos の共有リンクの形ではありません。')
      const now = Date.now()
      const row: StoredProject = {
        id: randomUUID(),
        name: name.trim() || '新しいプロジェクト',
        source: { kind: 'amazon', key: keyFor(source), url: shareUrl.trim() },
        photoCount: 0,
        status: 'new',
        createdAt: now,
        updatedAt: now,
        burstThreshold: null,
        burstThresholdLearnedAt: null
      }
      await store.putProject(row)
      return asProject(row)
    },

    exportAmazon,

    deleteProject: async (projectId: string) => {
      linkCache.delete(projectId)
      noRelay.delete(projectId)
      const rows = await store.photosOfProject(projectId)
      const ids = rows.map(row => row.id)
      await store.deleteProject(projectId, ids)
      await blobStore.remove(ids)
      for (const row of rows) {
        originals.delete(row.id)
        sourceIO.picker.delete(row.relativePath)
      }
    },

    getProjectPhotoPage: async (
      projectId: string, offset = 0, limit = 80,
      rating: number | null = null, sort: PhotoSort = 'name'
    ): Promise<PhotoPage> => {
      const rows = await store.photosOfProject(projectId)
      const matched = filterPhotos(rows.filter(row => !row.isMissing), rating).sort(comparePhotos(sort))
      // 画面に渡すのは 1 ページぶんだけ。全件を載せない。
      // 一覧のタイルはサムネイルしか使わない。表示用は読まない（拡大のときに 1 枚だけ作る）。
      const photos = await decorate(pagePhotos(matched, offset, limit), undefined, { display: false })
      return { photos, total: matched.length }
    },

    getPhotosByIds: async (projectId: string, photoIds: string[]) => {
      // 主キーで読む（全行を読まない）。渡された順を保つ。まとめの代表が先頭に来る前提の画面がある。
      const ordered = (await store.photosByIds(projectId, photoIds)).filter((row): row is StoredPhoto => !!row)
      return decorate(ordered, projectId)
    },

    getCoreInputs: async (projectId: string): Promise<Photo[]> => {
      const rows = await store.photosOfProject(projectId)
      // 4,000 枚でもサムネイルの実体は読まない。core が要るのは鍵・時刻・指紋だけ。
      return rows.filter(row => !row.isMissing).map(row => toPhoto(row, null, null, null))
    },

    saveSelectionResults: (projectId: string, entries: SelectionResult[]) =>
      store.saveSelectionResults(projectId, entries),

    resetSelectionResults: (projectId: string) => store.resetRatings(projectId),

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
      return store.moveRating(
        projectId, fromRating, toRating, includeIds === null ? null : new Set(includeIds), new Set(excludeIds)
      )
    },

    getSelectionSummary: async (projectId: string): Promise<SelectionSummary> =>
      summarizeRatings(await store.photosOfProject(projectId)),

    saveBurstThreshold: (projectId: string, threshold: number) =>
      store.patchProject(projectId, { burstThreshold: threshold, burstThresholdLearnedAt: Date.now() }),

    clearBurstThreshold: (projectId: string) =>
      store.patchProject(projectId, { burstThreshold: null, burstThresholdLearnedAt: null }),

    getPairOverrides: (projectId: string) => store.readPairOverrides(projectId),
    savePairOverrides: (projectId: string, overrides) => store.writePairOverrides(projectId, overrides),

    /**
     * フォルダ（handle・開発用）のプロジェクトを走査して準備する。**待たずに返す**（進捗は
     * `onProjectProgress`）。許可が無いフォルダは、ここ（ボタンの操作の中）で許可を求める。
     * ピッカーのプロジェクトには走査するフォルダが無いので何もしない。
     */
    startProjectScan: async (projectId: string) => {
      const row = await store.getProject(projectId)
      if (!row || sourceOf(row).kind === 'picker') return
      void (sourceOf(row).kind === 'amazon' ? prepareAmazon(projectId) : prepareFolder(projectId))
    },
    startBurstAnalysis: () => Promise.resolve(),

    /** リロードのあとの続き。許可が無ければ何もしない（画面に許可のボタンが出る）。 */
    startBackgroundAnalysis: async (projectId: string) => {
      const row = await store.getProject(projectId)
      if (!row || sourceOf(row).kind === 'picker' || preparing.has(projectId)) return
      if (sourceOf(row).kind === 'amazon') {
        // リンクが消えているものや、中継が無いと分かったものは叩き直さない。
        if (row.status === 'missing' || noRelay.has(projectId)) return
        preparing.add(projectId)
        cancelled.delete(projectId)
        try {
          await hashBacklog(projectId)
        } catch (cause) {
          emit(progressOf(projectId, 'background', 'error', 0, 0,
            cause instanceof Error ? cause.message : '解析できませんでした。'))
        } finally {
          preparing.delete(projectId)
        }
        return
      }
      const io = await ioFor(row, false).catch(() => null)
      if (!io) return
      preparing.add(projectId)
      cancelled.delete(projectId)
      try {
        await analyzeBacklog(projectId, io)
      } catch (cause) {
        emit(progressOf(projectId, 'background', 'error', 0, 0,
          cause instanceof Error ? cause.message : '解析できませんでした。'))
      } finally {
        preparing.delete(projectId)
      }
    },

    getAnalysisBacklog: async (projectId: string) => {
      if (noRelay.has(projectId)) return 0
      const rows = await store.photosOfProject(projectId)
      return rows.filter(row => !row.isMissing && row.dHash === null && row.analysisError === null).length
    },

    cancelProjectTask: (projectId: string) => {
      cancelled.add(projectId)
      return Promise.resolve()
    },

    saveSession: (projectId: string, selection: SavedSelection | null) =>
      store.writeSession(projectId, selection ? serializeSavedSelection(selection) : 'null'),
    loadSession: async (projectId: string) => {
      const raw = await store.readSession(projectId)
      if (!raw) return null
      // 読み戻しは core（wasm）が行う。旧版の形は null（最初から）。
      await initCore()
      return parseSavedSelection(raw)
    },

    sidecarSupported: async (projectId: string): Promise<SidecarAccess> =>
      (await (await sidecarIO(projectId))?.sidecarAccess()) ?? 'none',
    readSidecar: async (projectId: string) => (await sidecarIO(projectId))?.readSidecar() ?? null,
    writeSidecar: async (projectId: string, json: string, fileName = 'catalog.json') => {
      const io = await sidecarIO(projectId)
      if (!io || (await io.sidecarAccess()) !== 'readwrite') {
        throw new Error('この出所にはサイドカーを書けません。フォルダへのアクセスを許可してください。')
      }
      await io.writeSidecar(json, fileName)
    },
    writeSidecarChecked: async (projectId: string, json: string, expected: string | null) => {
      const io = await sidecarIO(projectId)
      if (!io || (await io.sidecarAccess()) !== 'readwrite') {
        throw new Error('この出所にはサイドカーを書けません。フォルダへのアクセスを許可してください。')
      }
      return io.writeSidecarChecked(json, expected)
    },
    loadSidecarState: (projectId: string): Promise<SidecarState> => store.readSidecarState(projectId),
    saveSidecarState: (projectId: string, state: SidecarState) => store.writeSidecarState(projectId, state),
    deviceIdentity: async (): Promise<DeviceIdentity> => ({ id: await store.deviceId(), name: 'ブラウザ' }),

    // ブラウザでは既に表示できる URL が入っている。
    photoUrl: (path: string) => path,
    // 拡大のとき、その 1 枚の URL を**そのとき作る**。一覧を読んだときに持っていた URL は、
    // 上限を超えて revoke されていることがある（W3）。
    photoOriginalUrl: async (photo: Photo) => {
      const file = originals.get(photo.id)
      if (file) return blobStore.originalUrl(photo.id, file)
      // Amazon（https）など、revoke されない URL はそのまま。
      if (!photo.path.startsWith('blob:')) return photo.path
      const found = (await blobStore.load([photo.id])).get(photo.id)
      return found?.displayUrl ?? found?.thumbnailUrl ?? photo.path
    },
    photoThumbnailUrl: (photo: Photo) => photo.thumbnailPath ?? photo.path,
    photoDisplayUrl: (photo: Photo) => photo.displayPath ?? photo.thumbnailPath ?? photo.path,

    // ブラウザでは表示用画像を準備のときに、作成時に選んだ長辺で作る。**原本を保存しない**
    // ので、あとから別の大きさで作り直すことはできない（`canRebuild: false`）。
    // 変えられるのはこれから作る分だけ。
    // Amazon は URL で出す（画像を端末に置かない）ので、選んだ長辺を保存するだけで変えられる。
    getDisplaySettings: async (projectId?: string) => {
      const saved = await store.readDisplayEdge().catch(() => null)
      const row = projectId ? await store.getProject(projectId) : null
      const amazon = !!row && sourceOf(row).kind === 'amazon'
      return {
        edge: saved ? nearestDisplayEdge(saved) : DISPLAY_EDGE_DEFAULT,
        choices: [...DISPLAY_EDGE_CHOICES],
        defaultEdge: DISPLAY_EDGE_DEFAULT,
        largeEdge: 1536,
        canRebuild: amazon,
        rebuildsOnChange: !amazon,
        projectEdge: projectId ? await displayEdgeOf(projectId) : undefined
      }
    },
    saveDisplayEdge: async (edge: number) => {
      const normalized = nearestDisplayEdge(edge)
      await store.writeDisplayEdge(normalized)
      return normalized
    },
    saveProjectDisplayEdge: async (projectId: string, edge: number | null) => {
      if (edge !== null) await store.patchProject(projectId, { displayEdge: nearestDisplayEdge(edge) })
      return displayEdgeOf(projectId)
    },
    // 準備と同時に作っているので、あとから溜まる分は無い。
    getDisplayBacklog: () => Promise.resolve(0),
    startDisplayGeneration: () => Promise.resolve(),
    resetDisplayImages: () => Promise.resolve(),

    exportPhotos: (): Promise<ExportReport> => unsupported('フォルダ分け'),
    writeRatingsToPhotos: (): Promise<ExportReport> => unsupported('メタデータへの書き込み'),
    saveCsv: async (fileName: string, text: string) => {
      downloadBlob(new Blob([text], { type: 'text/csv' }), fileName)
      return true
    },

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

      // 別のアルバムの同名ファイル（IMG_0001.JPG）でも、写真の鍵が重ならないようにする。
      const before = await store.photosOfProject(projectId)
      const keys = uniquePaths(
        files.map(file => file.webkitRelativePath || file.name),
        before.map(row => row.relativePath)
      )
      const jobs: AnalysisJob[] = []
      const rows = files.map((file, index) => {
        const id = randomUUID()
        const key = keys[index]!
        originals.set(id, file)
        sourceIO.picker.add(key, file)
        jobs.push({ id, file })
        return newRow(projectId, id, key, file.name)
      })
      await store.putPhotos(rows)
      await store.patchProject(projectId, { photoCount: before.length + rows.length, status: 'ready' })
      emit(progressOf(projectId, 'scan', 'complete', total, total, '読み込みが終わりました'))

      await analyze(projectId, jobs)
    },

    /** 共有・書き出しに使える原本。ピッカーのときだけで、リロード後は空になる。 */
    originalFile: (photoId: string) => originals.get(photoId) ?? null
  }
}
