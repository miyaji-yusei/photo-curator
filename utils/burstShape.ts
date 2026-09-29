/**
 * 見直した「こう分かれていてほしい」という形から、手直し（`PairOverride`）を作る。
 *
 * 保存するのは形そのものではなく、**core の基準との食い違いだけ**。
 * - 同じ塊なのに基準では別 → `join`
 * - 別の塊なのに基準では同じ → `split`
 * - 一致するなら、その 2 枚の既存の手直しは消す
 *
 * こうすると、切ってから元に戻しても無意味な手直しが溜まらず、
 * 同じ形を何度保存しても結果が変わらない（冪等）。
 * 基準の判定（`core.isSameBurst`）は呼び出し側から渡す。ここでは判断しない。
 */
import type { PairOverride, PhotoRef } from '~/lib/core'

export function overridesFromShape(
  run: PhotoRef[],
  blocks: string[][],
  existing: PairOverride[],
  isSameByThreshold: (left: PhotoRef, right: PhotoRef) => boolean
): PairOverride[] {
  const blockOf = new Map<string, number>()
  blocks.forEach((block, index) => {
    for (const path of block) blockOf.set(path, index)
  })
  const key = (left: string, right: string) => `${left}\u0000${right}`
  const next = new Map<string, PairOverride>()
  for (const item of existing) next.set(key(item.left, item.right), item)

  for (let index = 0; index + 1 < run.length; index += 1) {
    const left = run[index]!
    const right = run[index + 1]!
    const pair = key(left.relative_path, right.relative_path)
    const leftBlock = blockOf.get(left.relative_path)
    const wanted = leftBlock !== undefined && leftBlock === blockOf.get(right.relative_path)
    const raw = isSameByThreshold(left, right)
    if (wanted === raw) {
      next.delete(pair)
    } else {
      next.set(pair, {
        left: left.relative_path,
        right: right.relative_path,
        decision: wanted ? 'join' : 'split'
      })
    }
  }
  return [...next.values()]
}
