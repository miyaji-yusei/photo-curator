// 一覧の並べ替え・絞り込みに使う小さな型だけ残す（utils/photoQuery.ts が使う）。
// 判断の型（Session・PhotoRef など）は lib/core.ts、プロジェクト・写真の
// 保存形は types/project.ts（設計 07 章 段2）。

/** 星の上限。 */
export const MAX_RATING = 5

export type PhotoSort = 'rating' | 'name'

/** `counts[n]` が★n の枚数（0〜5）。 */
export interface SelectionSummary {
  counts: number[]
  total: number
}
