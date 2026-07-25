/**
 * レートの移動で「どの写真を動かすか」を表す選択状態。
 *
 * id の集合をそのまま持たない。既定が全選択で、対象が 5,000 枚あることも
 * あるので、id を並べる持ち方だとダイアログを開いた瞬間に全件をフロントへ
 * 載せることになる。**全選択（または全解除）からの差分**だけを持てば、
 * 利用者が実際に触った枚数しか持たずに済む。
 */
export interface MoveSelection {
  /** true なら「差分を除いた全件」、false なら「差分そのもの」が対象。 */
  selectAll: boolean
  /** 基準から外した（または加えた）写真の id。 */
  deviations: Set<string>
}

export function createMoveSelection(): MoveSelection {
  return { selectAll: true, deviations: new Set() }
}

/** その写真が移動の対象かどうか。基準と差分の排他的論理和。 */
export function isSelected(selection: MoveSelection, photoId: string) {
  return selection.selectAll !== selection.deviations.has(photoId)
}

export function toggleSelection(selection: MoveSelection, photoId: string): MoveSelection {
  const deviations = new Set(selection.deviations)
  if (deviations.has(photoId)) deviations.delete(photoId)
  else deviations.add(photoId)
  return { selectAll: selection.selectAll, deviations }
}

/** 全選択・全解除は、差分の基準そのものを切り替えて差分を捨てる。 */
export function setSelectAll(selectAll: boolean): MoveSelection {
  return { selectAll, deviations: new Set() }
}

/**
 * 移動する枚数。`total` はその星の**全件数**で、まだ画面に読み込んでいない
 * ぶんも含む。表示中の枚数から数えると、ページングの途中で実数とずれる。
 */
export function selectedCount(selection: MoveSelection, total: number) {
  return selection.selectAll ? total - selection.deviations.size : selection.deviations.size
}

/**
 * `move_rating` へ渡す引数へ落とす。
 * 全選択のときは「除外」を、全解除からの積み上げのときは「明示指定」を送る。
 * どちらの場合も、送る id は利用者が触った数だけになる。
 */
export function toMoveArgs(selection: MoveSelection): {
  includeIds: string[] | null
  excludeIds: string[]
} {
  const deviations = [...selection.deviations]
  return selection.selectAll
    ? { includeIds: null, excludeIds: deviations }
    : { includeIds: deviations, excludeIds: [] }
}
