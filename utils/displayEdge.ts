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
