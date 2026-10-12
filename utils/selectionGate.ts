/**
 * 「選別を始められるか」の条件（R3）。
 *
 * 以前は `useCurator` の 4 つの入口と `ProjectView` の開始ボタンに、少しずつ違う条件が散っていた。
 * 条件をここに 1 つの関数として集める。**入口ごとに見ている条件が違う**（挙動を変えないため）ので、
 * 各入口は**自分が今見ている事実だけ**を渡す。渡さなかった（`undefined`）事実は判定に使わない。
 *
 * | 入口 | 渡す事実 |
 * | --- | --- |
 * | `enterMethod`・開始ボタン | `photoCount`・`scanRunning`・`sidecarClash`・`sidecarChecking` |
 * | `beginTournament`・`startTournament` | `hasProject`・`taskDialog`・`scanRunning`・`sidecarClash` |
 * | `resumeSession` | `scanRunning`・`sidecarClash`・`sidecarChecking` |
 *
 * 走査中（`scanRunning`）はどの入口も見る（設計書 01 章「走査中は開始できない」。11 章 §4 B9）。
 */
export interface SelectionGateFacts {
  /** プロジェクトを開いているか。 */
  hasProject?: boolean
  /** 写真の枚数。0（や未定）なら始められない。 */
  photoCount?: number
  /** 走査などの進捗ダイアログが開いているか。 */
  taskDialog?: boolean
  /** 写真の走査（再読み込みを含む）の最中か。 */
  scanRunning?: boolean
  /** 別の端末の記録との食い違いが未解決か。 */
  sidecarClash?: boolean
  /** サイドカーを確かめている最中か。 */
  sidecarChecking?: boolean
}

/** 渡された事実のどれも始められない理由に当たらなければ true。 */
export function canStartSelection(facts: SelectionGateFacts): boolean {
  if (facts.hasProject === false) return false
  if (facts.photoCount !== undefined && !facts.photoCount) return false
  if (facts.taskDialog) return false
  if (facts.scanRunning) return false
  if (facts.sidecarClash) return false
  if (facts.sidecarChecking) return false
  return true
}
