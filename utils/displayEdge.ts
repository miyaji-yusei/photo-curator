/**
 * 表示用画像の長辺の選択肢と、容量の見込み。PC の `DISPLAY_EDGES`（Rust）と同じ値。
 * 容量は長辺の 2 乗に比例するとみなし、1024px で 2,000 枚あたり約 200MB を基準にする。
 */
export const DISPLAY_EDGE_CHOICES = [768, 1024, 1280, 1536, 1920] as const
export const DISPLAY_EDGE_BASE = 1024
/** 1024px・2,000 枚あたりの容量（MB）。 */
export const DISPLAY_MB_AT_BASE = 200
export const DISPLAY_ESTIMATE_PHOTOS = 2000

/** 2,000 枚あたりの容量の見込み（MB、10MB 単位に丸める）。 */
export function estimateDisplayMegabytes(edge: number): number {
  const scale = (edge / DISPLAY_EDGE_BASE) ** 2
  return Math.round((DISPLAY_MB_AT_BASE * scale) / 10) * 10
}

/** 選択肢に添える文。例: `1024px（2,000 枚あたり約 200MB）`。 */
export function displayEdgeLabel(edge: number): string {
  return `${edge}px（${DISPLAY_ESTIMATE_PHOTOS.toLocaleString('en-US')} 枚あたり約 ${estimateDisplayMegabytes(edge)}MB）`
}

/** 選択肢に無い値は、いちばん近いものに寄せる（Rust の `normalize_display_edge` と同じ考え）。 */
export function nearestDisplayEdge(edge: number, choices: readonly number[] = DISPLAY_EDGE_CHOICES): number {
  return choices.reduce((best, choice) => (Math.abs(choice - edge) < Math.abs(best - edge) ? choice : best), choices[0]!)
}

/** プロジェクトの画面から表示用画像の px を変えたときの扱い。 */
export type DisplayEdgePlan =
  /** 変えられない（原本が無く、作り直せない）。 */
  | 'locked'
  /** 同じ値。何もしない。 */
  | 'same'
  /** 警告なしで保存だけ（まだ何も作られていない・作り直しが要らない）。 */
  | 'save'
  /** 作り直しになる。「よろしいですか」を出してから。 */
  | 'confirm'

export interface DisplayEdgePlanInput {
  current: number
  next: number
  /** 作り直せるか（DisplaySettings.canRebuild。未指定は true）。 */
  canRebuild?: boolean
  /** 長辺を変えると画像を作り直すか（DisplaySettings.rebuildsOnChange。未指定は true）。 */
  rebuildsOnChange?: boolean
  /** 今の長辺で、すでに作られた表示用画像の枚数。 */
  builtCount: number
}

/** 変えられるか。原本が無く作り直せないときは false（画面は項目を押せなくし、理由を出す）。 */
export function canChangeDisplayEdge(canRebuild?: boolean): boolean {
  return canRebuild !== false
}

/** 作られた枚数。写真の数から、今の長辺でまだ要る枚数を引く。 */
export function builtDisplayCount(photoCount: number, backlog: number): number {
  return Math.max(0, photoCount - Math.max(0, backlog))
}

/** px を変えるときの扱い。再作成が要るのは、すでに作られた画像があり、値が変わり、作り直す環境のときだけ。 */
export function displayEdgePlan(input: DisplayEdgePlanInput): DisplayEdgePlan {
  if (!canChangeDisplayEdge(input.canRebuild)) return 'locked'
  if (input.next === input.current) return 'same'
  if (input.rebuildsOnChange === false) return 'save'
  return input.builtCount > 0 ? 'confirm' : 'save'
}

/** 警告ダイアログの本文に出す、作り直しの説明。 */
export function displayRebuildNote(isAmazon: boolean): string {
  return isAmazon
    ? 'Amazon Photos から写真を取り直すので、通信と時間がかかります。'
    : '写真を読み直すので、時間がかかります（小さくするときは一瞬です）。'
}
