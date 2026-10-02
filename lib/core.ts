// core（Rust）の判断を PC・Web の画面から呼ぶ、型付きの窓口。
//
// **判断そのものはここに書かない。** core-wasm（core-wasm/pkg、
// `pnpm core:wasm` で作る）をそのまま呼ぶだけ。
//
// フィールド名は core の JSON（`session_to_json` の出力）と揃えて
// スネークケースのまま（設計 04 章 note）。サイドカーのトップレベルだけ、
// 03 章のとおりキャメルケース（`updatedAt` など）。
//
// 呼ぶ前に必ず `init()`（ブラウザ）か `initWithBytes()`（Node・テスト）を
// 1 回呼ぶこと。2 回目以降は同じ Promise を返すので、何度呼んでも安全。

import wasmInit, { initSync } from '../core-wasm/pkg/photo_curator_core_wasm.js'
import * as wasm from '../core-wasm/pkg/photo_curator_core_wasm.js'
import type { InitInput, SyncInitInput } from '../core-wasm/pkg/photo_curator_core_wasm.js'

// ---------------------------------------------------------------------------
// 型（core の dictionary / Session と同じ形）
// ---------------------------------------------------------------------------

export interface PhotoRef {
  relative_path: string
  captured_at: number | null
  d_hash: string | null
  d_hash_version: number
}

export interface BurstThreshold {
  window_ms: number
  distance: number
  d_hash_version: number
}

export interface PairOverride {
  left: string
  right: string
  /** "join" | "split" */
  decision: string
}

export interface BurstGroup {
  members: string[]
  representative: string
}

export interface BurstAnswer {
  distance: number
  same: boolean
}

export interface Topped {
  path: string
  previous: number
}

export interface Decision {
  group: string[]
  chosen: string[]
  topped: Topped | null
  /** 確定の前の星の控え。空（または無い）なら古い形で、差分で戻す */
  before: Record<string, number>
}

export interface Session {
  group_size: number
  target_star: number
  round: number
  queue: string[]
  current: string[]
  survivors: string[]
  ratings: Record<string, number>
  members: Record<string, string[]>
  history: Decision[]
  finished: boolean
}

// ---- サイドカー（UDL には無い。core にだけある） ----

export interface SidecarPhoto {
  rating: number
}

export interface SidecarSessions {
  tournament?: Session
}

export interface Sidecar {
  version: number
  updatedAt: number
  updatedBy: string
  updatedByName: string
  photos: Record<string, SidecarPhoto>
  burstOverrides: PairOverride[]
  sessions: SidecarSessions
  burstDistance?: number
  // ---- v2（U33）の項目。すべて省略できる（古い catalog.json には無い）。 ----
  /** 書くたびに作る乱数。版の見分け。 */
  writeId?: string | null
  /** 書いた端末が見ていた版。 */
  basedOn?: string | null
  /** これまでの版（新しい順）。早送りの判定に使う。 */
  lineage?: string[] | null
  /** やり直しの世代。 */
  epoch?: string | null
  /** `"folder"` ＝ 写真の鍵が「選んだフォルダからの相対・`/` 区切り」。 */
  keyBase?: string | null
  /** 要約（判断には使わない）。 */
  progress?: SidecarProgress | null
}

// ---- サイドカー同期（U33。core の sidecar_sync。形は core の serde のまま） ----

/** 選別状況の要約（ダイアログの 1 行・catalog.json の `progress`）。 */
export interface SidecarProgress {
  started: boolean
  round: number
  finished: boolean
  /** この ROUND で決めた組の数。 */
  decided: number
  /** まだ見ていない写真の数。 */
  remaining: number
  /** ★1 以上の数。 */
  starred: number
  total?: number | null
  /** 手直しの数。 */
  overrides: number
  /** 連写の境目を学習したか。 */
  learned: boolean
}

/** 選別状況の正規形（比べるための形。鍵はフォルダ形式）。中身は core に任せ、TS では触らない。 */
export interface Judgement {
  stars: { path: string, rating: number }[]
  session: unknown | null
  overrides: PairOverride[]
  burst_distance: number | null
  epoch: string | null
}

/** 端末の控え（最後に読んだ／書いた版）。一度も見ていなければ token は空。 */
export interface SeenRecord {
  token: string
  key: string
  epoch: string | null
}

export type SettledReason = 'Nothing' | 'Same' | 'NoChange' | 'Detached' | 'ReadOnly' | 'NewerVersion'
export type PushReason = 'NoSidecar' | 'LocalChanged' | 'TheirsUntouched' | 'MineHasMore'
export type PullReason = 'LocalUntouched' | 'FastForward' | 'TheirsHasMore'
export type ClashReason = 'Diverged' | 'TheirsRestarted' | 'MineRestarted' | 'ExtrasConflict'
/** この端末から見て（Ahead＝この端末の方が進んでいる）。 */
export type ProgressOrder = 'Ahead' | 'Behind' | 'Even' | 'Unclear'
export type MergeMode = 'Intersection' | 'Union'

export interface MergePreview {
  mine_starred: number
  theirs_starred: number
  intersection_starred: number
  union_starred: number
  undecided: number
  mid_round: boolean
}

export interface MergeResult {
  session: Session
  ratings: Record<string, number>
  overrides: PairOverride[]
  burst_distance: number | null
  epoch: string | null
  starred: number
  undecided: number
}

export interface KeyCoverage {
  matched: number
  total: number
}

/** 開き方の判断（設計書 §4.3）。 */
export type SidecarPlan =
  | { Settled: { seen: SeenRecord | null, reason: SettledReason } }
  | { Push: { expected: string | null, aside_theirs: boolean, reason: PushReason } }
  | { Pull: { theirs: Sidecar, aside_mine: boolean, seen: SeenRecord, reason: PullReason } }
  | {
    Clash: {
      theirs: Sidecar
      mine_progress: SidecarProgress
      theirs_progress: SidecarProgress
      order: ProgressOrder
      reason: ClashReason
      preview: MergePreview
    }
  }

export type SidecarSync =
  | 'Settled'
  | 'Push'
  | { Pull: Sidecar }
  | { Clash: Sidecar }

// ---------------------------------------------------------------------------
// 初期化。**1 回だけ。**
// ---------------------------------------------------------------------------

let ready: Promise<void> | null = null
let readySync = false

/**
 * ブラウザ向け。`input` を省略すると、wasm-bindgen の既定（fetch）に任せる。
 * 2 回目以降は同じ Promise を返す（呼び直しても安全）。
 */
export function init(input?: InitInput | Promise<InitInput>): Promise<void> {
  if (!ready) {
    ready = wasmInit(input).then(() => {
      readySync = true
    })
  }
  return ready
}

/**
 * Node（vitest）向け。fetch を経由せず、読み込んだバイト列をそのまま渡す。
 * `core/tests/fixtures/` と cargo test が同じ答えになることを確かめるテストで使う。
 */
export function initWithBytes(bytes: SyncInitInput): void {
  if (readySync) return
  initSync({ module: bytes })
  readySync = true
  ready = Promise.resolve()
}

function ensureReady(): void {
  if (!readySync) {
    throw new Error('core が初期化されていません。先に init() / initWithBytes() を呼んでください。')
  }
}

// ---------------------------------------------------------------------------
// 連写
// ---------------------------------------------------------------------------

export function hashDistance(left: string, right: string): number {
  ensureReady()
  return wasm.hashDistance(left, right)
}

export function isSameBurst(pair: { left: PhotoRef; right: PhotoRef }, threshold: BurstThreshold): boolean {
  ensureReady()
  return wasm.isSameBurst(pair, threshold)
}

export function groupBursts(
  photos: PhotoRef[],
  threshold: BurstThreshold,
  overrides: PairOverride[]
): BurstGroup[] {
  ensureReady()
  return wasm.groupBursts(photos, threshold, overrides)
}

export function dHashFromLuma(luma: Uint8Array | number[]): string | null {
  ensureReady()
  return wasm.dHashFromLuma(luma instanceof Uint8Array ? luma : Uint8Array.from(luma)) ?? null
}

export function dHashFromGray(gray: Uint8Array | number[], width: number, height: number): string | null {
  ensureReady()
  return wasm.dHashFromGray(gray instanceof Uint8Array ? gray : Uint8Array.from(gray), width, height) ?? null
}

export function learnDistance(answers: BurstAnswer[], fallback: number): number {
  ensureReady()
  return wasm.learnDistance(answers, fallback)
}

// ---------------------------------------------------------------------------
// 選別
// ---------------------------------------------------------------------------

export function startRound(
  photos: PhotoRef[],
  groupSize: number,
  targetStar: number,
  groupBurstsOn: boolean,
  threshold: BurstThreshold,
  overrides: PairOverride[]
): Session {
  ensureReady()
  return wasm.startRound(photos, groupSize, targetStar, groupBurstsOn, threshold, overrides)
}

export function advance(session: Session, selected: string[]): Session {
  ensureReady()
  return wasm.advance(session, selected)
}

export function undo(session: Session): Session {
  ensureReady()
  return wasm.undo(session)
}

export function keepTop(session: Session, path: string): Session {
  ensureReady()
  return wasm.keepTop(session, path)
}

export function keepAndTop(session: Session, selected: string[], path: string): Session {
  ensureReady()
  return wasm.keepAndTop(session, selected, path)
}

export function setRepresentative(session: Session, shown: string, wanted: string): Session | null {
  ensureReady()
  return wasm.setRepresentative(session, shown, wanted) ?? null
}

export function resize(session: Session, groupSize: number): Session {
  ensureReady()
  return wasm.resize(session, groupSize)
}

export function regroup(
  session: Session,
  photos: PhotoRef[],
  groupBurstsOn: boolean,
  threshold: BurstThreshold,
  overrides: PairOverride[]
): Session {
  ensureReady()
  return wasm.regroup(session, photos, groupBurstsOn, threshold, overrides)
}

export function nextRound(
  previous: Session,
  photos: PhotoRef[],
  groupBurstsOn: boolean,
  threshold: BurstThreshold,
  overrides: PairOverride[]
): Session | null {
  ensureReady()
  return wasm.nextRound(previous, photos, groupBurstsOn, threshold, overrides) ?? null
}

export function roundFor(
  previous: Session,
  photos: PhotoRef[],
  star: number,
  groupBurstsOn: boolean,
  threshold: BurstThreshold,
  overrides: PairOverride[]
): Session | null {
  ensureReady()
  return wasm.roundFor(previous, photos, star, groupBurstsOn, threshold, overrides) ?? null
}

// ---------------------------------------------------------------------------
// 保存
// ---------------------------------------------------------------------------

export function sessionToJson(session: Session): string {
  ensureReady()
  return wasm.sessionToJson(session)
}

export function sessionFromJson(json: string): Session | null {
  ensureReady()
  return wasm.sessionFromJson(json) ?? null
}

// ---------------------------------------------------------------------------
// サイドカー
// ---------------------------------------------------------------------------

export function sidecarToJson(sidecar: Sidecar): string {
  ensureReady()
  return wasm.sidecarToJson(sidecar)
}

export function sidecarFromJson(json: string): Sidecar | null {
  ensureReady()
  return wasm.sidecarFromJson(json) ?? null
}

export function sidecarDecide(
  seenAt: number,
  seenBy: string,
  localChanged: boolean,
  remote: Sidecar | null
): SidecarSync {
  ensureReady()
  return wasm.sidecarDecide(seenAt, seenBy, localChanged, remote ?? null)
}

// ---- サイドカー同期（U33）。判断は core。ここは型を付けて呼ぶだけ。 ----

/** 写真の鍵をそろえる（区切りを `/`、Unicode を NFC）。 */
export function normalizeKey(key: string): string {
  ensureReady()
  return wasm.normalizeKey(key)
}

export function sidecarJudgement(sidecar: Sidecar): Judgement {
  ensureReady()
  return wasm.sidecarJudgement(sidecar)
}

export function judgementEquivalent(a: Judgement, b: Judgement): boolean {
  ensureReady()
  return wasm.judgementEquivalent(a, b)
}

/** 比較キー（`j1:` ＋ 16 進）。画像のハッシュ値とは関係が無い。 */
export function judgementKey(judgement: Judgement): string {
  ensureReady()
  return wasm.judgementKey(judgement)
}

export function isUntouched(judgement: Judgement): boolean {
  ensureReady()
  return wasm.isUntouched(judgement)
}

export function judgementProgress(judgement: Judgement): SidecarProgress {
  ensureReady()
  return wasm.judgementProgress(judgement)
}

export function sidecarToken(sidecar: Sidecar): string {
  ensureReady()
  return wasm.sidecarToken(sidecar)
}

export function sidecarSeen(sidecar: Sidecar): SeenRecord {
  ensureReady()
  return wasm.sidecarSeen(sidecar)
}

/**
 * 開き方の判断。`remote` は読めた catalog.json（鍵は `sidecarNormalizeKeys` 済み）か null。
 * **つながらない・壊れているときは呼ばない。**
 */
export function sidecarPlan(
  seen: SeenRecord, local: Judgement, remote: Sidecar | null, writable: boolean, detached: boolean
): SidecarPlan {
  ensureReady()
  return wasm.sidecarPlan(seen, local, remote ?? null, writable, detached)
}

/** 書く直前に v2 の印（version・writeId・basedOn・lineage・keyBase・progress）を入れる。 */
export function sidecarStamp(sidecar: Sidecar, writeId: string, base: Sidecar | null): Sidecar {
  ensureReady()
  return wasm.sidecarStamp(sidecar, writeId, base ?? null)
}

export function mergeJudgements(
  mine: Judgement, theirs: Judgement, mode: MergeMode, groupSize: number, freshEpoch: string
): MergeResult {
  ensureReady()
  return wasm.mergeJudgements(mine, theirs, mode, groupSize, freshEpoch)
}

export function mergePreview(mine: Judgement, theirs: Judgement): MergePreview {
  ensureReady()
  return wasm.mergePreview(mine, theirs)
}

/** 端末の鍵 → サイドカーの鍵（選んだフォルダからの相対・`/`・NFC）。PC・Web は prefix が空。 */
export function sidecarKeysToFolder(sidecar: Sidecar, prefix: string): Sidecar {
  ensureReady()
  return wasm.sidecarKeysToFolder(sidecar, prefix)
}

/** サイドカーの鍵 → 端末の鍵。Windows の PC は separator を `\`。 */
export function sidecarKeysFromFolder(sidecar: Sidecar, prefix: string, separator: string): Sidecar {
  ensureReady()
  return wasm.sidecarKeysFromFolder(sidecar, prefix, separator)
}

/** 読んだ直後に鍵をフォルダ形式にそろえる（古い Android の形は、選んだフォルダのパスから推定して外す）。 */
export function sidecarNormalizeKeys(sidecar: Sidecar, folderHint: string): Sidecar {
  ensureReady()
  return wasm.sidecarNormalizeKeys(sidecar, folderHint)
}

export function sidecarKeyCoverage(sidecar: Sidecar, photoKeys: string[]): KeyCoverage {
  ensureReady()
  return wasm.sidecarKeyCoverage(sidecar, photoKeys)
}
