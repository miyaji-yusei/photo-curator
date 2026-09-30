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

/**
 * 「この写真をまとめる」。選んだ代表（と、それぞれの連写の仲間）を、**撮影順で最初から最後まで**、
 * 隣どうしを全部 `join` の手直しにする。core は隣どうしのペアしか見ないため、離れた 2 枚を
 * まとめるにはあいだも連鎖させるしかない。**あいだに挟まる選んでいない写真も同じまとまりに入る**
 * （仕様として受け入れる。07 章）。
 *
 * - `order`: 全写真の relativePath（撮影順）
 * - `reps`: 選んだ代表の relativePath
 * - `members`: core の Session の `members`（代表 → 仲間）
 * - 既存の手直しのうち、今回の隣どうしと同じ 2 枚のものは置き換える
 * - 対象が 2 枚に満たなければ null（何もしない）
 */
export function joinSpanOverrides(
  order: string[],
  reps: string[],
  members: Record<string, string[]>,
  existing: PairOverride[]
): PairOverride[] | null {
  const wanted = new Set<string>()
  for (const rep of reps) {
    for (const mate of members[rep] ?? [rep]) wanted.add(mate)
  }
  let first = -1
  let last = -1
  order.forEach((path, index) => {
    if (!wanted.has(path)) return
    if (first < 0) first = index
    last = index
  })
  if (first < 0 || first === last) return null
  const span = order.slice(first, last + 1)
  const joins: PairOverride[] = []
  for (let index = 0; index + 1 < span.length; index += 1) {
    joins.push({ left: span[index]!, right: span[index + 1]!, decision: 'join' })
  }
  const replaced = new Set(joins.map(item => `${item.left}\u0000${item.right}`))
  return [...existing.filter(item => !replaced.has(`${item.left}\u0000${item.right}`)), ...joins]
}
