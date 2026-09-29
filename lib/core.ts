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
