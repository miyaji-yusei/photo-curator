/**
 * サイドカー（写真のフォルダ直下の `.photo-curator/catalog.json`）との同期（設計 03 章・U34）。
 *
 * - **判断は core の `sidecarPlan` だけ**が行う（設計書「サイドカー同期の調査と直し方の設計」§4.3）。
 *   ここは「読む → 判断に渡す → 結果どおりに動く」だけ。時刻の大小では決めない。
 *   - 意味が同じ → 何もしない（警告も出さない。控えだけ NAS の版に進める）
 *   - 端末が未着手 → 確認なしに取り込む／NAS が未着手で端末が着手済み → 書く（NAS の版は退避）
 *   - 早送り（NAS の版が、端末の見た版から続けたもの）→ 確認なしに取り込む
 *   - 両方とも着手済みで違う → 確認（5 択のダイアログ）
 * - 「端末が変わったか」は印ではなく「見た版の比較キー（`seenKey`）と今の比較キーが違うか」で決める。
 *   古い控え（`seenAt`・`seenBy`・`localChanged`）しか無いときは、そこから `legacy:` の控えを作る。
 * - 書き込みは楽観ロック（backend の `writeSidecarChecked`: ロック → 読んで見た版と同じか → 一時ファイル →
 *   置き換え → 読み戻し）。違えば書かずに判定し直す。
 * - 端末の中では、サイドカーの読み書きと控えの保存を**プロジェクトごとに 1 本の列**に並べる（`serialized`）。
 * - 写真の鍵は、書くとき「選んだフォルダからの相対・`/` 区切り」にし、読んだら core で同じ形にそろえ、
 *   取り込むときに端末の形（Windows の PC は `\`）へ戻す。
 *
 * `createSidecarSync(backend)` は Vue に依存しない（テストは偽の backend で動かす）。
 * `useSidecarSync()` はそれを画面の状態（refs）に繋ぐ。
 *
 * 用語: 「選別状況」＝星・選別の途中（Session）・連写の手直し・学習した境目。「比較キー」は選別状況から作る値で、
 * 画像のハッシュ値とは関係が無い。
 */
import * as core from '~/lib/core'
import type {
  ClashReason, Judgement, MergeMode, MergePreview, ProgressOrder, PullReason, PushReason, SeenRecord, Session,
  SettingValueBool, SettledReason, Sidecar, SidecarProgress
} from '~/lib/core'
import type { PhotoBackend, SidecarAccess, SidecarState } from '~/composables/photoBackend'
import type { Photo } from '~/types/photo'
import type { SavedSelection } from '~/utils/selectionFlow'
import { applyChanges } from '~/utils/ratingEdit'
import { alignFolderHint, asideTag, relocalizeKeys } from '~/utils/sidecarAside'

/** 新しく書くサイドカーの `version`（core の `SIDECAR_VERSION` と同じ）。 */
export const SIDECAR_VERSION = 2

/** 判定し直す回数の上限（書く直前に別の端末が書いた、が続いたとき）。 */
const MAX_ATTEMPTS = 3

export type SidecarBackend = Pick<PhotoBackend,
  | 'sidecarSupported' | 'readSidecar' | 'asideSidecar' | 'asideLocal' | 'writeSidecarChecked' | 'loadSidecarState'
  | 'saveSidecarState' | 'deviceIdentity' | 'getCoreInputs' | 'saveSelectionResults' | 'getPairOverrides'
  | 'savePairOverrides' | 'loadSession' | 'saveSession' | 'listProjects' | 'saveBurstThreshold'
  | 'clearBurstThreshold' | 'saveProjectPairRaw'>
  // 欠損の行の星（U52 D13）。無い backend（古い偽物）では欠損の星を載せない（従来どおり）。
  & Partial<Pick<PhotoBackend, 'getMissingRatings'>>

/** 古い要約（表示用。今は使っていないが、外から使えるよう残す）。 */
export interface SidecarSummary {
  /** ★1 以上の枚数。 */
  starred: number
  round: number
  updatedAt: number
  deviceName: string
}

/** 食い違いのダイアログに渡すもの。 */
export interface ClashInfo {
  /** NAS の版（鍵はフォルダ形式）。 */
  theirs: Sidecar
  /** ダイアログを出したときに読んだ catalog.json の中身（答えたときに変わっていないか確かめる）。 */
  theirsText: string
  mineName: string
  theirsName: string
  /** NAS の版を書いた端末の時刻（表示だけに使う）。 */
  theirsAt: number
  mineProgress: SidecarProgress
  theirsProgress: SidecarProgress
  /** この端末から見て（Ahead＝この端末の方が進んでいる）。 */
  order: ProgressOrder
  reason: ClashReason
  preview: MergePreview
  access: SidecarAccess
}

export type ClashChoice = 'theirs' | 'keep' | 'mine' | 'intersection' | 'union'

/**
 * プロジェクトの設定（U48: settings.pairRawJpeg）の結果。どの結果にも付きうる（選別状況の判断とは別）。
 * - `settingsAdopted`: ほかの端末の設定を取り込んだ（値。true＝オン）。写真に反映するには再走査が要る
 * - `settingsPushed`: 選別状況は同じで、設定だけをサイドカーに書いた
 */
export interface SettingsNote {
  settingsAdopted?: boolean
  settingsPushed?: boolean
}

export type SyncOutcome = SettingsNote & (
  | { kind: 'skipped', access: SidecarAccess }
  | { kind: 'settled', access: SidecarAccess, reason: SettledReason }
  | { kind: 'pushed', access: SidecarAccess, reason: PushReason }
  | { kind: 'pulled', access: SidecarAccess, reason: PullReason, from: string }
  | { kind: 'clash', access: SidecarAccess, clash: ClashInfo }
  /** 自動の書き込みのとき、取り込みと確認は次に開いたときへ回した。 */
  | { kind: 'deferred', access: SidecarAccess }
  /** ほかの端末が書いている（ロック）。次の契機に回した。 */
  | { kind: 'locked', access: SidecarAccess }
  /** 書く直前に何度も別の端末が書いた。次の契機に回した。 */
  | { kind: 'busy', access: SidecarAccess }
  /** 写真の場所が違う記録（鍵の一致が半分未満）。取り込まなかった。 */
  | { kind: 'mismatch', access: SidecarAccess, matched: number, total: number }
  /** 載せる写真の行が 0 件。空の記録で共有を上書きしない。 */
  | { kind: 'empty', access: SidecarAccess }
)

/** 互換のための別名。 */
export type OpenOutcome = SyncOutcome

export type ResolveResult =
  | { kind: 'done' }
  /** ダイアログを出したあとに NAS が変わった。実行せずに判定し直した（新しい結果）。 */
  | { kind: 'changed', outcome: SyncOutcome }
  /** 書けなかった（つながらない・書けない共有だった）。何も変えていない。 */
  | { kind: 'failed', reason: string, access: SidecarAccess }

export function summarize(sidecar: Sidecar): SidecarSummary {
  return {
    starred: Object.values(sidecar.photos).filter(photo => photo.rating >= 1).length,
    round: sidecar.sessions.tournament?.round ?? 0,
    updatedAt: sidecar.updatedAt,
    deviceName: sidecar.updatedByName
  }
}

/** ほかの端末の設定（U48）を取り込んだときのお知らせ。自動では走査しないので、再読み込みを促す。 */
export function settingsAdoptedNotice(value: boolean): string {
  return `ほかの端末の設定に合わせて「同名の JPEG と RAW を 1 枚として扱う」を${value ? 'オン' : 'オフ'}にしました。`
    + '写真を反映するには「写真を再読み込み」を押してください。'
}

/** サイドカーの Session から、この端末の「選別の途中」の封筒を作る。 */
export function envelopeFromSidecar(sidecar: Sidecar, session: Session): SavedSelection {
  const groupBursts = Object.values(session.members).some(members => members.length > 1)
  return {
    v: 2,
    core: session,
    stage: session.finished ? 'result' : 'tournament',
    settings: { groupSize: session.group_size, groupBursts },
    multiSelect: false,
    selectedInGroup: [],
    learning: null,
    burstDistance: groupBursts ? sidecar.burstDistance ?? null : null,
    updatedAt: Date.now()
  }
}

/** 版の見分けに使う乱数。 */
function randomId(): string {
  const crypto = globalThis.crypto
  if (crypto?.randomUUID) return crypto.randomUUID()
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}-${Math.random().toString(36).slice(2)}`
}

/** 控えの、古い形からの読み替え。`localKey` は今の端末の比較キー（古い印が「変更なし」のとき使う）。 */
export function seenOf(state: SidecarState, localKey: string): SeenRecord {
  if (typeof state.seenToken === 'string') {
    return { token: state.seenToken, key: state.seenKey ?? '', epoch: state.seenEpoch ?? null }
  }
  // 古い形: seenAt/seenBy は、その版の updatedAt/updatedBy（core の legacy: の見分けと同じ形）。
  const never = !state.seenAt && !state.seenBy
  return {
    token: never ? '' : `legacy:${state.seenAt}:${state.seenBy}`,
    // 古い印が立っていれば「分からない＝変更あり」、立っていなければ今の中身が見た版のまま。
    key: state.localChanged ? '' : localKey,
    epoch: null
  }
}

/**
 * core の鍵の推定（`sidecarNormalizeKeys`）に渡す「選んだフォルダのパス」（U52 D15）。古い形（keyBase が
 * folder でない）の記録だけ、鍵の頭と大文字小文字・正規化だけが違えば鍵の綴りに合わせる。
 */
function folderHint(sidecar: Sidecar, folderPath: string): string {
  if (sidecar.keyBase === 'folder') return folderPath
  const keys = [
    ...Object.keys(sidecar.photos),
    ...Object.keys(sidecar.sessions.tournament?.ratings ?? {}),
    ...sidecar.burstOverrides.flatMap(item => [item.left, item.right])
  ]
  return alignFolderHint(folderPath, keys)
}

interface ProjectRef { id: string }

/** 判断のための、いまの両側の写し。 */
interface Snapshot {
  access: SidecarAccess
  /** 読んだ catalog.json の中身（無ければ null）。楽観ロックの「見た版」。 */
  text: string | null
  /** NAS の版（鍵はフォルダ形式）。 */
  remote: Sidecar | null
  /** 端末の分（端末の鍵）。 */
  mineRaw: Sidecar
  /** 端末の分（フォルダ形式の鍵）。 */
  mine: Sidecar
  local: Judgement
  localKey: string
  state: SidecarState
  seen: SeenRecord
  /** 端末の鍵の区切り（Windows の PC は `\`）。 */
  separator: string
  /** 端末の写真の鍵（フォルダ形式）。 */
  photoKeys: string[]
  /** 載せる写真のうち、欠損でない行の数（0 なら書かない）。 */
  liveRows: number
  /**
   * `mineRaw` を組んだ写真の行（R9）。**このあとに行を書いていない間だけ**組み立てに渡してよい
   * （取り込み・混ぜるで `applyLocal` が★を書いたあとは古い）。
   */
  rows: Photo[]
  identity: { id: string, name: string }
  /**
   * 設定（U48）をどうするか。`AdoptRemote` のときは `mine` の設定をもう NAS の値にしてある
   * （どの書き込みでも、古い端末の値で NAS の新しい値を上書きしない）。
   */
  settings: core.SettingsPlan
  /** 端末の設定（`pairRawJpeg`）。`at` が 0 なら一度も切り替えていない。 */
  localPair: SettingValueBool
}

export interface SidecarSync {
  /** 開いたとき・選別を始める前・「今すぐ保存」の確認。書く・取り込むはここで片付け、食い違いだけ返す。 */
  checkOnOpen: (project: ProjectRef) => Promise<SyncOutcome>
  /** 自動の書き込み（背面へ回る・窓を閉じる・ホームへ戻る・ラウンドの終わり）。取り込みと確認は次へ回す。書いたら true。 */
  pushIfChanged: (project: ProjectRef) => Promise<boolean>
  /** 自動の書き込みの結果を全部返す版。 */
  pushAuto: (project: ProjectRef) => Promise<SyncOutcome>
  /** 相手の記録を取り込む（星・Session・手直し・距離）。相手に無い Session・距離は、端末が未着手でない限り消さない。 */
  adopt: (project: ProjectRef, sidecar: Sidecar) => Promise<void>
  /** 食い違いの 5 択を実行する。 */
  resolveClash: (project: ProjectRef, clash: ClashInfo, choice: ClashChoice) => Promise<ResolveResult>
  /** 切り離し中（B のあと）に「NAS に書き込む」。NAS の版を退避して端末の分を書く。 */
  writeToNas: (project: ProjectRef) => Promise<boolean>
  /** 古い形の「判断が変わった」印（古い版のアプリに戻したとき用）。書き込みの列に並ぶ。 */
  markChanged: (projectId: string) => Promise<void>
  /** 「最初からやり直す」。この端末の選別状況の世代を新しくする。 */
  markRestarted: (projectId: string) => Promise<void>
  /**
   * 端末の分から Sidecar を組む（端末の鍵のまま）。`rows` は組む直前に同じ処理の中で読んだ写真の行だけを渡す
   * （R9。渡さなければ自分で読む。古い行を渡すと★や手直しの最新が欠ける）。
   */
  buildSidecar: (project: ProjectRef, updatedAt?: number, rows?: Photo[]) => Promise<Sidecar>
  /** 切り離し中か。 */
  isDetached: (projectId: string) => Promise<boolean>
  access: (projectId: string) => Promise<SidecarAccess>
}

export function createSidecarSync(backend: SidecarBackend, now: () => number = Date.now): SidecarSync {
  /** 古い印がすでに付いていると分かっているプロジェクト（保存を繰り返さない）。 */
  const knownChanged = new Set<string>()
  /** プロジェクトごとの読み書きの直列化。 */
  const chains = new Map<string, Promise<unknown>>()

  function serialized<T>(projectId: string, task: () => Promise<T>): Promise<T> {
    const previous = chains.get(projectId) ?? Promise.resolve()
    const next = previous.catch(() => undefined).then(task)
    chains.set(projectId, next)
    return next
  }

  async function access(projectId: string): Promise<SidecarAccess> {
    try {
      return await backend.sidecarSupported(projectId)
    } catch {
      return 'none'
    }
  }

  async function saveState(projectId: string, state: SidecarState) {
    await backend.saveSidecarState(projectId, state)
    if (!state.localChanged) knownChanged.delete(projectId)
  }

  /**
   * 端末の分を組む。`given` は写真の行（`getCoreInputs` の結果）で、**組む直前に同じ処理の中で読んだもの
   * だけ**を渡す（R9）。渡さなければ今どおりここで読む。古い行を渡すと、★や手直しの最新が欠けた版を組む。
   */
  async function buildWith(project: ProjectRef, updatedAt?: number, given?: Photo[]) {
    const [identity, rows, overrides, saved, projects, state] = await Promise.all([
      backend.deviceIdentity(),
      given ?? backend.getCoreInputs(project.id),
      backend.getPairOverrides(project.id),
      backend.loadSession(project.id),
      backend.listProjects(),
      backend.loadSidecarState(project.id)
    ])
    // 星は写真の行が持っている（結果・移動・書き出しが読む）。全部載せる。
    const photos: Sidecar['photos'] = {}
    for (const row of rows) photos[row.relativePath] = { rating: row.rating }
    // 一時的に見えない（欠損の印の）写真の星も載せる（U52 D13）。「見えない」は「星を 0 にした」ではない。
    // 載せないと、セッションの無いプロジェクトでは比較キーが変わって書き、NAS と相手の端末から星が消える。
    const listed = backend.getMissingRatings ? await backend.getMissingRatings(project.id) : null
    const missing = Array.isArray(listed) ? listed : []
    for (const row of missing) {
      if (row.rating >= 1 && !(row.relativePath in photos)) photos[row.relativePath] = { rating: row.rating }
    }
    const info = projects.find(item => item.id === project.id)
    const distance = info?.burstThreshold
    const sidecar: Sidecar = {
      version: SIDECAR_VERSION,
      updatedAt: updatedAt ?? (saved?.updatedAt || now()),
      updatedBy: identity.id,
      updatedByName: identity.name,
      photos,
      burstOverrides: overrides,
      sessions: saved ? { tournament: saved.core } : {}
    }
    if (typeof distance === 'number') sidecar.burstDistance = distance
    if (state.localEpoch) sidecar.epoch = state.localEpoch
    // プロジェクトの設定（U48）。一度も切り替えていなくても値は書く（そのとき at は 0）。
    const pair: SettingValueBool = { value: info?.pairRawJpeg !== false, at: info?.pairRawJpegAt ?? 0 }
    sidecar.settings = { pairRawJpeg: pair }
    const separator = rows.some(row => row.relativePath.includes('\\')) ? '\\' : '/'
    return { sidecar, identity, rows, saved, state, folderPath: info?.folderPath ?? '', separator, pair }
  }

  async function buildSidecar(project: ProjectRef, updatedAt?: number, rows?: Photo[]): Promise<Sidecar> {
    return (await buildWith(project, updatedAt, rows)).sidecar
  }

  /**
   * 端末の今の比較キー（取り込んだ・混ぜたあとの控えに使う）。行は渡さずに読み直す（R9: 直前の `applyLocal` が
   * ★を書いたので、それより前に読んだ行は古い）。
   */
  async function currentKey(project: ProjectRef): Promise<string> {
    const { sidecar } = await buildWith(project)
    return core.judgementKey(core.sidecarJudgement(core.sidecarKeysToFolder(sidecar, '')))
  }

  /**
   * 両側を読んで写しを作る。**読めない（つながらない）なら投げる**（判断に渡さない）。
   * 壊れている catalog.json は、無いものとして扱って上書きしない（投げる）。
   */
  async function snapshot(project: ProjectRef, level: SidecarAccess): Promise<Snapshot> {
    const text = await backend.readSidecar(project.id)
    const parsed = text === null ? null : core.sidecarFromJson(text)
    if (text !== null && parsed === null) {
      throw new Error('サイドカー（.photo-curator/catalog.json）の形式を読めませんでした。')
    }
    const built = await buildWith(project, now())
    let mine = core.sidecarKeysToFolder(built.sidecar, '')
    const local = core.sidecarJudgement(mine)
    const localKey = core.judgementKey(local)
    // 設定（U48）は選別状況と別に決める（確認は出さない。新しく切り替えた方）。
    const settings = core.settingsResolve(mine.settings ?? null, parsed?.settings ?? null)
    if (typeof settings === 'object' && 'AdoptRemote' in settings) {
      mine = { ...mine, settings: { ...(mine.settings ?? {}), pairRawJpeg: { ...settings.AdoptRemote } } }
    }
    return {
      settings,
      localPair: built.pair,
      access: level,
      text,
      remote: parsed ? core.sidecarNormalizeKeys(parsed, folderHint(parsed, built.folderPath)) : null,
      mineRaw: built.sidecar,
      mine,
      local,
      localKey,
      state: built.state,
      seen: seenOf(built.state, localKey),
      separator: built.separator,
      photoKeys: Object.keys(mine.photos),
      liveRows: built.rows.length,
      rows: built.rows,
      identity: built.identity
    }
  }

  /** 控えを書いた／読んだ版にする。古い 3 つも合わせて書く（古い版のアプリに戻したとき用）。 */
  function stateFor(previous: SidecarState, version: Sidecar, seen: SeenRecord, extra: Partial<SidecarState> = {}): SidecarState {
    return {
      ...previous,
      seenAt: version.updatedAt,
      seenBy: version.updatedBy,
      localChanged: false,
      seenToken: seen.token,
      seenKey: seen.key,
      seenEpoch: seen.epoch,
      localEpoch: previous.localEpoch ?? null,
      detached: false,
      ...extra
    }
  }

  /**
   * 端末の分を楽観ロックで書く。`base` は置き換える NAS の版（無ければ null）、`expected` はその中身。
   * `aside` なら、backend が**ロックを取って見た版と同じことを確かめたあとで**、置き換える NAS の版を
   * `catalog.<相手>.<時刻>.json` へ退避する（退避が書けなければ書かない。U52 D4。Android の U44 と同じ順）。
   */
  async function writeMine(
    project: ProjectRef, snap: Snapshot, aside: boolean
  ): Promise<'written' | 'changed' | 'locked' | 'empty'> {
    // 欠損でない行が 0 件（例: 全部が欠損扱いの間）なら書かない。空に近い記録で共有を上書きしない。
    if (snap.liveRows === 0) return 'empty'
    const asideOwner = aside && snap.text !== null && snap.remote ? asideTag(snap.remote.updatedBy) : null
    const writeId = randomId()
    const stamped = core.sidecarStamp(snap.mine, writeId, snap.remote)
    const result = await backend.writeSidecarChecked(project.id, core.sidecarToJson(stamped), snap.text, asideOwner)
    if (result !== 'written') return result
    // 控えの比較キーは「書いた写し」のもの。書いている間に増えた判断は、次に変更ありとして残る。
    await saveState(project.id, stateFor(snap.state, stamped, {
      token: writeId, key: snap.localKey, epoch: stamped.epoch ?? null
    }))
    return 'written'
  }

  /**
   * 置き換える前の端末の分を控える（取り込む・混ぜる前。U52 D4）。**控えられなければ投げる**（置き換えない）。
   * 端末の中（PC はアプリのデータフォルダの `aside/`、Web は IndexedDB。最新 5 つ）には必ず、
   * `nas` なら NAS の `catalog.<自分>.<時刻>.json` にも。
   */
  async function asideMine(project: ProjectRef, snap: Snapshot, nas: boolean) {
    const json = core.sidecarToJson(snap.mine)
    await backend.asideLocal(project.id, json)
    if (nas) await backend.asideSidecar(project.id, json, asideTag(snap.identity.id))
  }

  /** 端末の鍵の Sidecar を、端末の選別状況に入れる（星・Session・手直し・距離）。 */
  async function applyLocal(project: ProjectRef, received: Sidecar, untouched: boolean) {
    const projectId = project.id
    const rows = await backend.getCoreInputs(projectId)
    // 鍵の Unicode の正規化だけが違う端末の行の名前に合わせる（U52 D15。NAS の鍵は NFC）。
    const incoming = relocalizeKeys(received, rows.map(row => row.relativePath))
    const tournament = incoming.sessions.tournament ?? null
    // 星の出どころは core の正規形と同じ: Session があれば Session の星、無ければ行の星（photos）。
    const stars: Record<string, number> = {}
    if (tournament) for (const [path, rating] of Object.entries(tournament.ratings)) stars[path] = rating
    else for (const [path, photo] of Object.entries(incoming.photos)) stars[path] = photo.rating
    if (tournament) {
      await backend.saveSession(projectId, envelopeFromSidecar(incoming, tournament))
    } else if (untouched) {
      await backend.saveSession(projectId, null)
    } else {
      // 相手に Session が無くても、端末の進んだ Session は消さない（U34 の仕様の変更）。
      // 星だけ取り込んだ値にそろえる（開いたときの自己修復で、行の星が Session に戻されないように）。
      const saved = await backend.loadSession(projectId)
      if (saved) {
        const changes = Object.fromEntries(rows.map(row => [row.relativePath, stars[row.relativePath] ?? 0]))
        await backend.saveSession(projectId, { ...saved, core: applyChanges(saved.core, changes), updatedAt: now() })
      }
    }
    await backend.savePairOverrides(projectId, incoming.burstOverrides)
    if (typeof incoming.burstDistance === 'number') await backend.saveBurstThreshold(projectId, incoming.burstDistance)
    else if (untouched) await backend.clearBurstThreshold(projectId)
    // 端末に学習した距離があり、相手に無ければ残す（Session と同じ扱い）。
    // 星を記録に合わせる。記録に無い写真は 0。変わる行だけ書く。
    const entries = rows
      .map(row => ({ id: row.id, rating: stars[row.relativePath] ?? 0, was: row.rating }))
      .filter(entry => entry.rating !== entry.was)
      .map(({ id, rating }) => ({ id, rating }))
    if (entries.length) await backend.saveSelectionResults(projectId, entries)
  }

  /**
   * NAS の版（フォルダ形式の鍵）を取り込み、控えをその版にする。端末が未着手でなければ、**先に控える**
   * （端末の中には必ず、`asideNas` で書ける共有なら NAS にも。控えられなければ取り込まない。U52 D4）。
   */
  async function pull(project: ProjectRef, snap: Snapshot, theirs: Sidecar, token: string, asideNas: boolean) {
    const untouched = core.isUntouched(snap.local)
    if (!untouched) await asideMine(project, snap, asideNas && snap.access === 'readwrite')
    await applyLocal(project, core.sidecarKeysFromFolder(theirs, '', snap.separator), untouched)
    // 控えの比較キーは「取り込んだあとの端末」のもの（端末に残した分があっても、変更ありと読み違えない）。
    const key = await currentKey(project)
    await saveState(project.id, stateFor(snap.state, theirs, { token, key, epoch: theirs.epoch ?? null }, {
      localEpoch: theirs.epoch ?? null
    }))
  }

  function coverageProblem(snap: Snapshot, theirs: Sidecar): { matched: number, total: number } | null {
    const coverage = core.sidecarKeyCoverage(theirs, snap.photoKeys)
    return coverage.total > 0 && coverage.matched * 2 < coverage.total ? coverage : null
  }

  function clashInfo(snap: Snapshot, plan: Extract<core.SidecarPlan, { Clash: unknown }>['Clash']): ClashInfo {
    return {
      theirs: plan.theirs,
      theirsText: snap.text ?? '',
      mineName: snap.identity.name,
      theirsName: plan.theirs.updatedByName,
      theirsAt: plan.theirs.updatedAt,
      mineProgress: plan.mine_progress,
      theirsProgress: plan.theirs_progress,
      order: plan.order,
      reason: plan.reason,
      preview: plan.preview,
      access: snap.access
    }
  }

  /**
   * 読む → 判断 → 実行。`full` は開いたとき（書く・取り込む・確認）、`auto` は自動の書き込み
   * （書くだけ。取り込みと確認は次に開いたときへ回す）。列の中で呼ぶこと。
   */
  async function syncNow(project: ProjectRef, mode: 'full' | 'auto'): Promise<SyncOutcome> {
    const level = await access(project.id)
    if (level === 'none') return { kind: 'skipped', access: level }
    // 設定（U48）を取り込んだら、その値（どの結果にも付ける。判定し直しても 1 回だけ取り込む）。
    let adopted: boolean | undefined
    const note = (outcome: SyncOutcome): SyncOutcome =>
      adopted === undefined ? outcome : { ...outcome, settingsAdopted: adopted }
    for (let attempt = 0; attempt < MAX_ATTEMPTS; attempt++) {
      const snap = await snapshot(project, level)
      // ほかの端末で新しく切り替えた設定は、確認なしに端末へ取り込む（開いたときだけ。自動の書き込みでは
      // 端末を変えず、書く版にだけ NAS の値を入れる）。写真への反映は再走査（ここでは走査しない）。
      if (mode === 'full' && typeof snap.settings === 'object' && 'AdoptRemote' in snap.settings) {
        const { value, at } = snap.settings.AdoptRemote
        await backend.saveProjectPairRaw(project.id, value, at)
        adopted = value
      }
      const plan = core.sidecarPlan(snap.seen, snap.local, snap.remote, level === 'readwrite', snap.state.detached === true)
      if ('Settled' in plan) {
        const { seen, reason } = plan.Settled
        if (seen && snap.remote) {
          // 意味が同じ。控えだけ NAS の版に進める（警告は出さない）。切り離しも終わる。
          await saveState(project.id, stateFor(snap.state, snap.remote, seen))
        } else if (reason === 'NoChange' && typeof snap.state.seenToken !== 'string') {
          // 古い形の控えのまま変わっていない。今の比較キーで新しい形に移しておく（以後は印に頼らない）。
          await saveState(project.id, {
            ...snap.state, localChanged: false, seenToken: snap.seen.token, seenKey: snap.seen.key, seenEpoch: null
          })
        }
        // 選別状況はそのままで、端末で切り替えた設定だけが NAS に無い・古い → 設定のために書く（U48）。
        if (settingsOnlyPush(snap, reason, level)) {
          const written = await writeMine(project, snap, false)
          if (written === 'changed') continue
          if (written === 'locked') return note({ kind: 'locked', access: level })
          return note(written === 'written'
            ? { kind: 'settled', access: level, reason, settingsPushed: true }
            : { kind: 'settled', access: level, reason })
        }
        return note({ kind: 'settled', access: level, reason })
      }
      if ('Push' in plan) {
        const written = await writeMine(project, snap, plan.Push.aside_theirs)
        if (written === 'written') return note({ kind: 'pushed', access: level, reason: plan.Push.reason })
        if (written === 'empty') return note({ kind: 'empty', access: level })
        if (written === 'locked') return note({ kind: 'locked', access: level })
        continue // 見た版と違った → 読み直して判定し直す
      }
      if (mode === 'auto') return note({ kind: 'deferred', access: level })
      if ('Pull' in plan) {
        const { theirs, aside_mine: asideFirst, seen, reason } = plan.Pull
        const problem = coverageProblem(snap, theirs)
        if (problem) return note({ kind: 'mismatch', access: level, ...problem })
        await pull(project, snap, theirs, seen.token, asideFirst)
        return note({ kind: 'pulled', access: level, reason, from: theirs.updatedByName })
      }
      return note({ kind: 'clash', access: level, clash: clashInfo(snap, plan.Clash) })
    }
    return note({ kind: 'busy', access: level })
  }

  /**
   * 選別状況では書かない（Settled）ときに、設定のためだけに書くか（U48）。
   * 端末で**切り替えたことがある**（at > 0）設定が NAS に無い・NAS より新しいときだけ。一度も切り替えていない
   * 既定の値のためだけには書かない（古い版の catalog.json を開くたびに書き換えない）。書けない共有・
   * 切り離し中・新しすぎる版は、選別状況と同じく書かない（それぞれ Settled の理由で分かる）。
   */
  function settingsOnlyPush(snap: Snapshot, reason: SettledReason, level: SidecarAccess): boolean {
    return snap.settings === 'PushLocal'
      && snap.localPair.at > 0
      && level === 'readwrite'
      && (reason === 'Same' || reason === 'NoChange' || reason === 'Nothing')
  }

  const checkOnOpen = (project: ProjectRef) => serialized(project.id, () => syncNow(project, 'full'))
  const pushAuto = (project: ProjectRef) => serialized(project.id, () => syncNow(project, 'auto'))
  const pushIfChanged = async (project: ProjectRef) => (await pushAuto(project)).kind === 'pushed'

  async function adopt(project: ProjectRef, sidecar: Sidecar): Promise<void> {
    await serialized(project.id, async () => {
      const snap = await snapshot(project, await access(project.id))
      // R9: 行は直前の snapshot が同じ列の中で読んだもの（間に書き込みは無い）。ここで使うのは folderPath だけ。
      const built = await buildWith(project, undefined, snap.rows)
      const theirs = core.sidecarNormalizeKeys(sidecar, folderHint(sidecar, built.folderPath))
      await pull(project, snap, theirs, core.sidecarToken(theirs), true)
    })
  }

  /** D・E: 両側から新しい 1 つを作る（フォルダ形式の鍵）。 */
  function merged(snap: Snapshot, theirs: Sidecar, mode: MergeMode): Sidecar {
    const groupSize = snap.mine.sessions.tournament?.group_size ?? theirs.sessions.tournament?.group_size ?? 4
    const result = core.mergeJudgements(snap.local, core.sidecarJudgement(theirs), mode, groupSize, randomId())
    const photos: Sidecar['photos'] = {}
    for (const key of snap.photoKeys) photos[key] = { rating: result.ratings[key] ?? 0 }
    const out: Sidecar = {
      ...snap.mine,
      photos,
      burstOverrides: result.overrides,
      sessions: { tournament: result.session }
    }
    delete out.burstDistance
    if (typeof result.burst_distance === 'number') out.burstDistance = result.burst_distance
    out.epoch = result.epoch
    return out
  }

  async function resolveClash(project: ProjectRef, clash: ClashInfo, choice: ClashChoice): Promise<ResolveResult> {
    return serialized(project.id, async () => {
      const level = await access(project.id)
      try {
        const snap = await snapshot(project, level)
        // ダイアログを出したあとに NAS が変わっていたら、実行せずに出し直す。
        if (snap.text !== clash.theirsText || !snap.remote) {
          return { kind: 'changed', outcome: await syncNow(project, 'full') } as const
        }
        const theirs = snap.remote
        const writable = level === 'readwrite'
        const needsWrite = choice === 'mine' || choice === 'intersection' || choice === 'union'
        if (needsWrite && !writable) {
          return { kind: 'failed', reason: 'この共有には書き込めません。', access: level } as const
        }
        if (choice === 'keep') {
          // B: NAS には触らず、この端末を切り離す。控えは NAS の版（また変われば聞き直す）。
          await saveState(project.id, stateFor(snap.state, theirs, core.sidecarSeen(theirs), { detached: true }))
          return { kind: 'done' } as const
        }
        if (choice === 'theirs') {
          const problem = coverageProblem(snap, theirs)
          if (problem) {
            return {
              kind: 'failed',
              reason: `写真の場所が違う記録のようです（一致 ${problem.matched}/${problem.total}）。取り込みませんでした。`,
              access: level
            } as const
          }
          // A: 端末の分を端末の中（と、書ける共有なら NAS）に控えてから取り込む（控えられなければ取り込まない）。
          await pull(project, snap, theirs, core.sidecarToken(theirs), true)
          return { kind: 'done' } as const
        }
        if (choice === 'mine') {
          // C: NAS の版を退避して、端末の分を書く。
          const written = await writeMine(project, snap, true)
          return afterWrite(project, written, level)
        }
        // D・E: 混ぜる。両方を退避し、混ぜた結果を書いてから端末にも入れる。
        // ROUND か対象の★が違うと混ぜない（ダイアログでも押せない。U45）。何も変えずに理由を返す。
        if (!core.mergePreview(snap.local, core.sidecarJudgement(theirs)).mergeable) {
          return { kind: 'failed', reason: 'ROUND か対象の★がほかの端末と違うので、混ぜられません。', access: level } as const
        }
        const mode: MergeMode = choice === 'intersection' ? 'Intersection' : 'Union'
        const next = merged(snap, theirs, mode)
        await asideMine(project, snap, true)
        const writeId = randomId()
        const stamped = core.sidecarStamp(next, writeId, theirs)
        const written = await backend.writeSidecarChecked(project.id, core.sidecarToJson(stamped), snap.text, asideTag(theirs.updatedBy))
        if (written !== 'written') return afterWrite(project, written, level)
        await applyLocal(project, core.sidecarKeysFromFolder(next, '', snap.separator), false)
        const key = await currentKey(project)
        await saveState(project.id, stateFor(snap.state, stamped, {
          token: writeId, key, epoch: stamped.epoch ?? null
        }, { localEpoch: stamped.epoch ?? null }))
        return { kind: 'done' } as const
      } catch (cause) {
        // 書けなかった。読むだけの共有と決めつけず、何も変えずに理由を返す（つながらないだけなら、選び直せる）。
        const reason = cause instanceof Error ? cause.message : '記録を書けませんでした。'
        return { kind: 'failed', reason, access: await access(project.id) } as const
      }
    })
  }

  async function afterWrite(
    project: ProjectRef, written: 'written' | 'changed' | 'locked' | 'empty', level: SidecarAccess
  ): Promise<ResolveResult> {
    if (written === 'written') return { kind: 'done' }
    if (written === 'locked') return { kind: 'failed', reason: 'ほかの端末が書き込んでいます。少し待ってから選んでください。', access: level }
    if (written === 'empty') return { kind: 'failed', reason: '写真の行が無いため、書きませんでした。', access: level }
    return { kind: 'changed', outcome: await syncNow(project, 'full') }
  }

  async function writeToNas(project: ProjectRef): Promise<boolean> {
    return serialized(project.id, async () => {
      const level = await access(project.id)
      if (level !== 'readwrite') return false
      for (let attempt = 0; attempt < MAX_ATTEMPTS; attempt++) {
        const snap = await snapshot(project, level)
        const same = snap.remote ? core.judgementEquivalent(snap.local, core.sidecarJudgement(snap.remote)) : false
        const written = await writeMine(project, snap, !same)
        if (written === 'written') return true
        if (written !== 'changed') return false
      }
      return false
    })
  }

  function markChanged(projectId: string): Promise<void> {
    if (knownChanged.has(projectId)) return Promise.resolve()
    // 列に並べる（書き終えた控えを、読んだ時点の古い値で上書きしない）。
    return serialized(projectId, async () => {
      const state = await backend.loadSidecarState(projectId)
      if (!state.localChanged) await backend.saveSidecarState(projectId, { ...state, localChanged: true })
      knownChanged.add(projectId)
    })
  }

  function markRestarted(projectId: string): Promise<void> {
    return serialized(projectId, async () => {
      const state = await backend.loadSidecarState(projectId)
      await backend.saveSidecarState(projectId, { ...state, localEpoch: `e-${randomId()}`, localChanged: true })
    })
  }

  async function isDetached(projectId: string): Promise<boolean> {
    return (await backend.loadSidecarState(projectId)).detached === true
  }

  return {
    checkOnOpen, pushIfChanged, pushAuto, adopt, resolveClash, writeToNas, markChanged, markRestarted,
    buildSidecar, isDetached, access
  }
}

// ---------------------------------------------------------------------------
// 画面の状態
// ---------------------------------------------------------------------------

/** 食い違いのダイアログに渡すもの。 */
export interface SidecarClash extends ClashInfo {
  projectId: string
}

/**
 * 画面が使うサイドカーの状態。`useCurator` が 1 つだけ持つ。
 * 自動の書き込み（背面へ回る・窓を閉じる・ホームへ戻る）の失敗は握りつぶさず `message` に出す。
 */
export function useSidecarSync(
  backend: PhotoBackend,
  hooks: {
    /** ほかの端末の設定（U48）を取り込んだ。画面が持っているプロジェクトを読み直す。 */
    onSettingsAdopted?: (projectId: string, value: boolean) => void | Promise<void>
  } = {}
) {
  const sync = createSidecarSync(backend)
  const access = ref<SidecarAccess>('none')
  const clash = ref<SidecarClash | null>(null)
  const busy = ref(false)
  /** 開いたとき・始める前の確認の最中。終わるまで「選別を開始・再開」は押させない（Android と同じ）。 */
  const checking = ref(false)
  /** 失敗の理由（赤）。 */
  const message = ref('')
  /** お知らせ（取り込んだ など）。 */
  const notice = ref('')
  /** 「この端末の状況を残す」を選んだあと（NAS とは別）。 */
  const detached = ref(false)
  /** 最後にこの端末が書いた時刻。表示用。 */
  const savedAt = ref<number | null>(null)

  async function refreshAccess(projectId: string) {
    access.value = await sync.access(projectId)
    detached.value = await sync.isDetached(projectId).catch(() => false)
  }

  function show(project: ProjectRef, outcome: SyncOutcome) {
    access.value = outcome.access
    if (outcome.kind === 'clash') clash.value = { projectId: project.id, ...outcome.clash }
    if (outcome.kind === 'pushed') savedAt.value = Date.now()
    if (outcome.kind === 'pulled') notice.value = `${outcome.from} の記録を取り込みました。`
    if (outcome.kind === 'mismatch') {
      message.value = `写真の場所が違う記録のようです（一致 ${outcome.matched}/${outcome.total}）。取り込みませんでした。`
    }
    if (outcome.kind === 'locked') message.value = 'ほかの端末が書き込んでいたため、今回は書きませんでした。'
    if (outcome.settingsAdopted !== undefined) {
      const adopted = settingsAdoptedNotice(outcome.settingsAdopted)
      notice.value = notice.value ? `${notice.value}${adopted}` : adopted
      void Promise.resolve(hooks.onSettingsAdopted?.(project.id, outcome.settingsAdopted)).catch(() => undefined)
    }
  }

  async function checkOnOpen(project: ProjectRef): Promise<SyncOutcome | null> {
    clash.value = null
    message.value = ''
    notice.value = ''
    checking.value = true
    try {
      const outcome = await sync.checkOnOpen(project)
      show(project, outcome)
      detached.value = await sync.isDetached(project.id).catch(() => false)
      return outcome
    } catch (cause) {
      // 読めない・つながらない。選別は続けられるので、理由だけ出す（判断はしない）。
      await refreshAccess(project.id).catch(() => undefined)
      message.value = cause instanceof Error ? cause.message : 'サイドカーを確認できませんでした。'
      return null
    } finally {
      checking.value = false
    }
  }

  /** 自動の書き込み。書けなくても画面は止めない。取り込みと確認は次に開いたときへ回す。 */
  async function pushAuto(project: ProjectRef | null) {
    if (!project || clash.value) return
    try {
      const outcome = await sync.pushAuto(project)
      if (outcome.kind === 'pushed' || outcome.settingsPushed) savedAt.value = Date.now()
    } catch (cause) {
      message.value = cause instanceof Error ? cause.message : 'サイドカーに書けませんでした。'
    }
  }

  /** 「今すぐ保存」。開いたときと同じ判断（食い違えばダイアログ）。 */
  async function saveNow(project: ProjectRef) {
    if (clash.value) return
    busy.value = true
    try {
      const outcome = await checkOnOpen(project)
      if (outcome?.kind === 'settled' && outcome.settingsPushed) {
        savedAt.value = Date.now()
        notice.value = notice.value || 'プロジェクトの設定をサイドカーに書き込みました。'
      } else if (outcome?.kind === 'settled' && outcome.reason !== 'ReadOnly' && outcome.settingsAdopted === undefined) {
        notice.value = '保存する変更はありません。'
      }
      return outcome
    } finally {
      busy.value = false
    }
  }

  /** 切り離し中の「NAS に書き込む」。 */
  async function writeToNas(project: ProjectRef) {
    busy.value = true
    message.value = ''
    try {
      if (await sync.writeToNas(project)) {
        savedAt.value = Date.now()
        detached.value = false
        notice.value = 'この端末の状況をサイドカーに書き込みました。'
      } else {
        message.value = 'サイドカーに書き込めませんでした。少し待ってからもう一度試してください。'
      }
    } catch (cause) {
      message.value = cause instanceof Error ? cause.message : 'サイドカーに書けませんでした。'
    } finally {
      busy.value = false
    }
  }

  /** 食い違いの答え。終わったら true（ダイアログを閉じる）。 */
  async function resolve(choice: ClashChoice): Promise<boolean> {
    const current = clash.value
    if (!current) return false
    busy.value = true
    message.value = ''
    try {
      const result = await sync.resolveClash({ id: current.projectId }, current, choice)
      if (result.kind === 'failed') {
        // 何も変えていない。ダイアログを残して選び直してもらう（書けない共有なら書く選択肢は消える）。
        access.value = result.access
        clash.value = { ...current, access: result.access }
        message.value = result.reason
        return false
      }
      clash.value = null
      if (result.kind === 'changed') {
        // NAS が変わっていた。新しい判断を出す（また食い違えば、新しい中身でダイアログ）。
        show({ id: current.projectId }, result.outcome)
        if (result.outcome.kind === 'clash') {
          message.value = '確認している間に NAS の記録が変わりました。もう一度選んでください。'
          return false
        }
      } else {
        if (choice === 'keep') {
          detached.value = true
          notice.value = 'この端末の状況を残しました（NAS とは別）。'
        } else {
          detached.value = false
          if (choice !== 'theirs') savedAt.value = Date.now()
          notice.value = choice === 'theirs' ? 'サイドカーの記録を取り込みました。' : 'サイドカーに書き込みました。'
        }
      }
      return true
    } catch (cause) {
      message.value = cause instanceof Error ? cause.message : '記録を切り替えられませんでした。'
      return false
    } finally {
      busy.value = false
    }
  }

  function markChanged(projectId: string) {
    sync.markChanged(projectId).catch((cause) => {
      message.value = cause instanceof Error ? cause.message : '変更の印を残せませんでした。'
    })
  }

  async function markRestarted(projectId: string) {
    await sync.markRestarted(projectId).catch(() => undefined)
  }

  return {
    sync, access, clash, busy, checking, message, notice, detached, savedAt, refreshAccess, checkOnOpen, pushAuto, saveNow,
    writeToNas, resolve, markChanged, markRestarted
  }
}

/**
 * 書き時のうち、画面の外で起きる 2 つ（アプリが背面へ回る・窓を閉じる）を登録する。
 * 戻り値で解除する。**ホームへ戻る**と**ラウンドの終わり**は呼ぶ側が `pushAuto` を呼ぶ。
 *
 * `beforeUnload` で終わりを待てるとは限らない。`visibilitychange`（hidden）で先に書くのが本命。
 * `flush` があれば（T14 の保存の待ち行列）、その完了を待ってから書く。
 */
export function registerAutoPush(
  push: () => Promise<void> | void, flush?: () => Promise<void>
): () => void {
  const run = () => {
    void (async () => {
      try {
        await flush?.()
      } catch {
        // 待ち行列の失敗はここで扱わない。書けるものは書く。
      }
      await push()
    })()
  }
  const onVisibility = () => {
    if (document.visibilityState === 'hidden') run()
  }
  document.addEventListener('visibilitychange', onVisibility)
  window.addEventListener('pagehide', run)
  window.addEventListener('beforeunload', run)
  return () => {
    document.removeEventListener('visibilitychange', onVisibility)
    window.removeEventListener('pagehide', run)
    window.removeEventListener('beforeunload', run)
  }
}
