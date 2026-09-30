/**
 * 結果の格子で、連写を 1 タイルに畳む。
 *
 * core の Session の `members`（代表 → 仲間）に入っている写真は、仲間のうち
 * **いま読み込んでいる行の中で星が一番高い 1 枚**だけをタイルにする。
 * 行はページ送りで来るので、畳むのは「読み込んだ行の中で」だけ（ページをまたいだ
 * 畳みはしない）。行は絞り込みで選ばれたものなので、星の高さを比べるだけで
 * 「絞り込みに合う中で一番高い 1 枚」になる。
 *
 * 星が同じときは、先に読み込んだ行（並びの早い方）を残す。
 */
export interface CollapsibleRow {
  relativePath: string
  rating: number
}

export interface BurstTile<T extends CollapsibleRow> {
  /** タイルに出す 1 枚。 */
  photo: T
  /** 連写の仲間の数（`⧉N` の N）。単独なら 1。 */
  burstSize: number
  /** 連写の仲間の全部（relativePath）。単独なら自分だけ。 */
  mates: string[]
}

export function collapseBursts<T extends CollapsibleRow>(
  rows: readonly T[],
  members: Record<string, string[]> | null | undefined
): BurstTile<T>[] {
  // 仲間の 1 枚 → その組の全員。1 人だけの組は畳まない。
  const groupOf = new Map<string, string[]>()
  for (const mates of Object.values(members ?? {})) {
    if (mates.length < 2) continue
    for (const path of mates) groupOf.set(path, mates)
  }

  const tiles: BurstTile<T>[] = []
  const tileOfGroup = new Map<string[], BurstTile<T>>()
  for (const row of rows) {
    const mates = groupOf.get(row.relativePath)
    if (!mates) {
      tiles.push({ photo: row, burstSize: 1, mates: [row.relativePath] })
      continue
    }
    const known = tileOfGroup.get(mates)
    if (!known) {
      const tile: BurstTile<T> = { photo: row, burstSize: mates.length, mates }
      tileOfGroup.set(mates, tile)
      tiles.push(tile)
    } else if (row.rating > known.photo.rating) {
      known.photo = row
    }
  }
  return tiles
}
