/**
 * サイドカー（写真のフォルダ直下の `.photo-curator/catalog.json`）との同期（設計 03 章）。
 *
 * - **4 通りの判断は core の `sidecarDecide` だけ**が行う。ここは「読む → 判断に渡す → 結果どおりに動く」だけ。
 * - 時刻の大小で勝敗を決めない。変更が無ければ書かない。
 *   書いたら `seenAt`・`seenBy` を必ず更新し、書けなかったら更新しない。
 * - 書くのは `localChanged` のときだけ（ラウンドの終わり・背面へ回る／窓を閉じる・ホームへ戻る・「今すぐ保存」）。
 * - 書けない出所（ピッカー・書き込みの許可が無いフォルダ・開発用）は、読めれば取り込むだけ。ダイアログは出さない。
 *
 * `createSidecarSync(backend)` は Vue に依存しない（テストは偽の backend で動かす）。
 * `useSidecarSync()` はそれを画面の状態（refs）に繋ぐ。
 */
import * as core from '~/lib/core'
import type { Session, Sidecar } from '~/lib/core'
import type { PhotoBackend, SidecarAccess } from '~/composables/photoBackend'
import type { SavedSelection } from '~/utils/selectionFlow'

/** サイドカーの `version`（core の形の版）。 */
export const SIDECAR_VERSION = 1

export type SidecarBackend = Pick<PhotoBackend,
  | 'sidecarSupported' | 'readSidecar' | 'writeSidecar' | 'loadSidecarState' | 'saveSidecarState'
  | 'deviceIdentity' | 'getCoreInputs' | 'saveSelectionResults' | 'getPairOverrides' | 'savePairOverrides'
  | 'loadSession' | 'saveSession' | 'listProjects' | 'saveBurstThreshold' | 'clearBurstThreshold'>

/** ダイアログに並べる要約。 */
export interface SidecarSummary {
  /** ★1 以上の枚数。 */
  starred: number
  round: number
  updatedAt: number
  deviceName: string
}

export type OpenOutcome =
  | { kind: 'skipped', access: SidecarAccess }
  | { kind: 'settled', access: SidecarAccess }
  | { kind: 'pushed', access: SidecarAccess }
  | { kind: 'pulled', access: SidecarAccess }
  | { kind: 'clash', access: SidecarAccess, theirs: Sidecar, mine: SidecarSummary, theirsSummary: SidecarSummary }

export function summarize(sidecar: Sidecar): SidecarSummary {
  return {
    starred: Object.values(sidecar.photos).filter(photo => photo.rating >= 1).length,
    round: sidecar.sessions.tournament?.round ?? 0,
    updatedAt: sidecar.updatedAt,
    deviceName: sidecar.updatedByName
  }
}

/** 退避のファイル名に使える文字だけにする（`catalog.<端末の id の先頭 12 文字>.json`）。 */
export function asideFileName(deviceId: string): string {
  const head = [...deviceId].slice(0, 12).join('').replace(/[^A-Za-z0-9-]/g, '-')
  return `catalog.${head || 'other'}.json`
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

export interface SidecarSync {
  /** 開いたときの確認。Push・Pull はここで片付け、Clash だけ呼び出し側にダイアログを出させる。 */
  checkOnOpen: (project: { id: string }) => Promise<OpenOutcome>
  /** `localChanged` のときだけ、今の状態から組んで書く。書いたら true。 */
  pushIfChanged: (project: { id: string }) => Promise<boolean>
  /** 相手の記録を取り込む（Session・手直し・距離・星）。 */
  adopt: (project: { id: string }, sidecar: Sidecar) => Promise<void>
  /** 「この端末の結果を使う」。相手を退避してから、この端末の分を書く。 */
  keepMine: (project: { id: string }, theirs: Sidecar) => Promise<void>
  /** 「NAS の記録を使う」。この端末の分を退避してから、相手の分を取り込む。 */
  keepTheirs: (project: { id: string }, theirs: Sidecar) => Promise<void>
  /** 判断が変わった印を付ける。同じ状態のあいだは書き込まない。 */
  markChanged: (projectId: string) => Promise<void>
  /** 端末の分から Sidecar を組む。 */
  buildSidecar: (project: { id: string }, updatedAt?: number) => Promise<Sidecar>
  access: (projectId: string) => Promise<SidecarAccess>
}

export function createSidecarSync(backend: SidecarBackend, now: () => number = Date.now): SidecarSync {
  /** 印を付けた回数。書いている最中に付いた印を、書き終わりに消さないため。 */
  const generation = new Map<string, number>()
  /** 印がすでに付いていると分かっているプロジェクト（保存を繰り返さない）。 */
  const knownChanged = new Set<string>()
  /** プロジェクトごとの書き込みの直列化。 */
  const chains = new Map<string, Promise<unknown>>()

  function serialized<T>(projectId: string, task: () => Promise<T>): Promise<T> {
    const previous = chains.get(projectId) ?? Promise.resolve()
    const next = previous.catch(() => undefined).then(task)
    chains.set(projectId, next)
    return next
  }

  const generationOf = (projectId: string) => generation.get(projectId) ?? 0

  async function access(projectId: string): Promise<SidecarAccess> {
    try {
      return await backend.sidecarSupported(projectId)
    } catch {
      return 'none'
    }
  }

  async function markChanged(projectId: string): Promise<void> {
    generation.set(projectId, generationOf(projectId) + 1)
    if (knownChanged.has(projectId)) return
    const state = await backend.loadSidecarState(projectId)
    if (!state.localChanged) await backend.saveSidecarState(projectId, { ...state, localChanged: true })
    knownChanged.add(projectId)
  }

  async function buildSidecar(project: { id: string }, updatedAt?: number): Promise<Sidecar> {
    const [identity, rows, overrides, saved, projects] = await Promise.all([
      backend.deviceIdentity(),
      backend.getCoreInputs(project.id),
      backend.getPairOverrides(project.id),
      backend.loadSession(project.id),
      backend.listProjects()
    ])
    // 星は写真の行が持っている（結果・移動・書き出しが読む）。全部載せる。
    const photos: Sidecar['photos'] = {}
    for (const row of rows) photos[row.relativePath] = { rating: row.rating }
    const distance = projects.find(item => item.id === project.id)?.burstThreshold
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
    return sidecar
  }

  async function writeOut(project: { id: string }): Promise<boolean> {
    const projectId = project.id
    if ((await access(projectId)) !== 'readwrite') return false
    const state = await backend.loadSidecarState(projectId)
    if (!state.localChanged) return false
    const before = generationOf(projectId)
    const sidecar = await buildSidecar(project, now())
    // 載せる行が 0 件（例: 全部が欠損扱いの間）なら書かない。空の記録で共有を上書きしない。
    // localChanged も落とさず、行が戻ったあとの書き出しに残す。
    if (Object.keys(sidecar.photos).length === 0) return false
    await backend.writeSidecar(projectId, core.sidecarToJson(sidecar))
    // 書けたときだけ印を更新する。書いている間にまた変わっていたら、変更ありのまま残す。
    const changedMeanwhile = generationOf(projectId) !== before
    await backend.saveSidecarState(projectId, {
      seenAt: sidecar.updatedAt,
      seenBy: sidecar.updatedBy,
      localChanged: changedMeanwhile
    })
    if (!changedMeanwhile) knownChanged.delete(projectId)
    return true
  }

  const pushIfChanged = (project: { id: string }) => serialized(project.id, () => writeOut(project))

  async function applyAdopt(project: { id: string }, sidecar: Sidecar): Promise<void> {
    const projectId = project.id
    const tournament = sidecar.sessions.tournament
    await backend.saveSession(projectId, tournament ? envelopeFromSidecar(sidecar, tournament) : null)
    await backend.savePairOverrides(projectId, sidecar.burstOverrides)
    if (typeof sidecar.burstDistance === 'number') await backend.saveBurstThreshold(projectId, sidecar.burstDistance)
    else await backend.clearBurstThreshold(projectId)
    // 星を記録に合わせる。記録に無い写真は 0。変わる行だけ書く。
    const rows = await backend.getCoreInputs(projectId)
    const entries = rows
      .map(row => ({ id: row.id, rating: sidecar.photos[row.relativePath]?.rating ?? 0, was: row.rating }))
      .filter(entry => entry.rating !== entry.was)
      .map(({ id, rating }) => ({ id, rating }))
    if (entries.length) await backend.saveSelectionResults(projectId, entries)
    await backend.saveSidecarState(projectId, {
      seenAt: sidecar.updatedAt, seenBy: sidecar.updatedBy, localChanged: false
    })
    knownChanged.delete(projectId)
  }

  const adopt = (project: { id: string }, sidecar: Sidecar) =>
    serialized(project.id, () => applyAdopt(project, sidecar))

  async function checkOnOpen(project: { id: string }): Promise<OpenOutcome> {
    const projectId = project.id
    const level = await access(projectId)
    if (level === 'none') return { kind: 'skipped', access: level }
    const json = await backend.readSidecar(projectId)
    const remote = json === null ? null : core.sidecarFromJson(json)
    // 読めない（壊れている・新しすぎる）ものを、無いものとして扱って上書きしない。
    if (json !== null && remote === null) {
      throw new Error('サイドカー（.photo-curator/catalog.json）の形式を読めませんでした。')
    }
    const state = await backend.loadSidecarState(projectId)
    // 書けない出所では、端末の分を書き戻せない。ダイアログで選ばせず、読めれば取り込むだけにする。
    const decision = core.sidecarDecide(
      state.seenAt, state.seenBy, level === 'readwrite' ? state.localChanged : false, remote
    )
    if (decision === 'Settled') return { kind: 'settled', access: level }
    if (decision === 'Push') {
      await pushIfChanged(project)
      return { kind: 'pushed', access: level }
    }
    if ('Pull' in decision) {
      await adopt(project, decision.Pull)
      return { kind: 'pulled', access: level }
    }
    const mine = summarize(await buildSidecar(project))
    return {
      kind: 'clash', access: level, theirs: decision.Clash, mine, theirsSummary: summarize(decision.Clash)
    }
  }

  async function keepMine(project: { id: string }, theirs: Sidecar): Promise<void> {
    const projectId = project.id
    await serialized(projectId, async () => {
      // 相手を捨てない。退避が書けなければ、ここで止める（自分の分も書かない）。
      await backend.writeSidecar(projectId, core.sidecarToJson(theirs), asideFileName(theirs.updatedBy))
    })
    await markChanged(projectId)
    await pushIfChanged(project)
  }

  async function keepTheirs(project: { id: string }, theirs: Sidecar): Promise<void> {
    const projectId = project.id
    await serialized(projectId, async () => {
      // この端末の分も捨てない。退避が書けなければ、取り込まない。
      const mine = await buildSidecar(project)
      await backend.writeSidecar(projectId, core.sidecarToJson(mine), asideFileName(mine.updatedBy))
      await applyAdopt(project, theirs)
    })
  }

  /**
   * 食い違いの答えを実行する。書き込みに失敗したら、その出所を書けない共有（readonly）として扱い直し、
   * 相手の記録を取り込む（仕様: 書けない共有ではダイアログを出さず、取り込むだけ）。
   * 取り込みまで失敗したときは投げる（呼び出し側がダイアログを残す）。
   */
  async function resolveClash(
    project: { id: string }, theirs: Sidecar, choice: 'mine' | 'theirs'
  ): Promise<{ kind: 'done' } | { kind: 'readonly', reason: string }> {
    try {
      if (choice === 'mine') await keepMine(project, theirs)
      else await keepTheirs(project, theirs)
      return { kind: 'done' }
    } catch (cause) {
      const reason = cause instanceof Error ? cause.message : '記録を書けませんでした。'
      await adopt(project, theirs)
      return { kind: 'readonly', reason }
    }
  }

  return { checkOnOpen, pushIfChanged, adopt, keepMine, keepTheirs, resolveClash, markChanged, buildSidecar, access }
}

// ---------------------------------------------------------------------------
// 画面の状態
// ---------------------------------------------------------------------------

/** 食い違いのダイアログに渡すもの。 */
export interface SidecarClash {
  projectId: string
  theirs: Sidecar
  mine: SidecarSummary
  theirsSummary: SidecarSummary
}

/**
 * 画面が使うサイドカーの状態。`useCurator` が 1 つだけ持つ。
 * 自動の書き込み（背面へ回る・窓を閉じる・ホームへ戻る）の失敗は握りつぶさず `message` に出す。
 */
export function useSidecarSync(backend: PhotoBackend) {
  const sync = createSidecarSync(backend)
  const access = ref<SidecarAccess>('none')
  const clash = ref<SidecarClash | null>(null)
  const busy = ref(false)
  const message = ref('')
  /** 最後にこの端末が書いた時刻。表示用。 */
  const savedAt = ref<number | null>(null)

  async function refreshAccess(projectId: string) {
    access.value = await sync.access(projectId)
  }

  async function checkOnOpen(project: { id: string }): Promise<OpenOutcome | null> {
    clash.value = null
    message.value = ''
    try {
      const outcome = await sync.checkOnOpen(project)
      access.value = outcome.access
      if (outcome.kind === 'clash') {
        clash.value = {
          projectId: project.id, theirs: outcome.theirs, mine: outcome.mine, theirsSummary: outcome.theirsSummary
        }
      }
      if (outcome.kind === 'pushed') savedAt.value = Date.now()
      return outcome
    } catch (cause) {
      // 読めない・つながらない。選別は続けられるので、理由だけ出す。
      await refreshAccess(project.id)
      message.value = cause instanceof Error ? cause.message : 'サイドカーを確認できませんでした。'
      return null
    }
  }

  /** 自動の書き込み。書けなくても画面は止めない。 */
  async function pushAuto(project: { id: string } | null) {
    if (!project || clash.value) return
    try {
      if (await sync.pushIfChanged(project)) savedAt.value = Date.now()
    } catch (cause) {
      message.value = cause instanceof Error ? cause.message : 'サイドカーに書けませんでした。'
    }
  }

  /** 「今すぐ保存」。 */
  async function saveNow(project: { id: string }) {
    if (clash.value) return
    busy.value = true
    message.value = ''
    try {
      if (await sync.pushIfChanged(project)) savedAt.value = Date.now()
      else message.value = '保存する変更はありません。'
    } catch (cause) {
      message.value = cause instanceof Error ? cause.message : 'サイドカーに書けませんでした。'
    } finally {
      busy.value = false
    }
  }

  async function resolve(choice: 'mine' | 'theirs'): Promise<boolean> {
    const current = clash.value
    if (!current) return false
    busy.value = true
    try {
      const result = await sync.resolveClash({ id: current.projectId }, current.theirs, choice)
      clash.value = null
      if (result.kind === 'readonly') {
        // 書けない共有だった。ダイアログを閉じ、読むだけの出所として相手を取り込んだ。
        access.value = 'readonly'
        message.value = `この共有には書き込めないため、共有側の記録を取り込みました（${result.reason}）`
      } else {
        savedAt.value = Date.now()
      }
      return true
    } catch (cause) {
      // 取り込みまで失敗したときは、選び直せるようダイアログを残す。
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

  return { sync, access, clash, busy, message, savedAt, refreshAccess, checkOnOpen, pushAuto, saveNow, resolve, markChanged }
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
