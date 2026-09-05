/**
 * まとまりの形を手で直すための、純粋な計算だけ。
 *
 * **考え方: まとまりは「鎖」で、編集とは鎖を切る／繋ぐこと。**
 *
 * 撮影順に並んだ1続きの写真（`run`）に対して、隣どうしの境目が `run.length - 1`
 * 個ある。`cuts[i]` が true なら `run[i]` と `run[i+1]` は別のまとまり。
 * この 1 つの表現で、分割・切り離し・結合・全解除がすべて表せる。
 *
 * - 1,2,3,4,5 を 1,2 と 3,4,5 に分ける … `cuts[1]` を立てる
 * - 3 をまとめから外す                  … `cuts[1]` と `cuts[2]` を立てる
 * - 近くの写真を取り込む                … その境目の cut を下ろす
 *
 * **まとまりは run 上で必ず連続している**。飛び飛びの集合にはならないので、
 * 境目の真偽値だけで状態が完全に決まる。
 */

/** まとまり = 写真 1 枚以上の塊。1 枚だけの塊は「独立した写真」を意味する。 */
export type Blocks = string[][]

/** `run` に対する境目の数。写真が 0 枚か 1 枚なら境目は無い。 */
export function boundaryCount(run: string[]): number {
  return Math.max(0, run.length - 1)
}

/** すべて繋がった状態（境目がひとつも無い）。 */
export function noCuts(run: string[]): boolean[] {
  return Array.from({ length: boundaryCount(run) }, () => false)
}

/**
 * いまのまとまり方から境目を起こす。
 * `groups` に一緒に入っていない隣どうしが、そのまま切れ目になる。
 * どのグループにも属さない写真は 1 枚のまとまりとして扱われる。
 */
export function cutsFromGroups(run: string[], groups: string[][]): boolean[] {
  const groupOf = new Map<string, number>()
  groups.forEach((group, index) => {
    for (const id of group) groupOf.set(id, index)
  })
  return Array.from({ length: boundaryCount(run) }, (_unused, index) => {
    const left = groupOf.get(run[index]!)
    const right = groupOf.get(run[index + 1]!)
    // どちらかがグループ外なら、その時点で別のまとまり。
    return left === undefined || right === undefined || left !== right
  })
}

/** 境目からまとまりを組み直す。表示にはこちらを使う。 */
export function blocksFromCuts(run: string[], cuts: boolean[]): Blocks {
  if (!run.length) return []
  const blocks: Blocks = [[run[0]!]]
  for (let index = 1; index < run.length; index += 1) {
    if (cuts[index - 1]) blocks.push([run[index]!])
    else blocks[blocks.length - 1]!.push(run[index]!)
  }
  return blocks
}

/**
 * 選んだ写真を、**連続した塊ごとに**1つのまとまりへ切り離す。
 *
 * 切るのは塊の**両端だけ**。内側は触らないので、3,4,5 を選べば 3,4,5 は
 * 1つのまとまりのまま切り出される（バラバラにはならない）。
 *
 * 飛び飛びに選んだときは塊が 1 枚ずつになり、結果としてそれぞれが独立する。
 * 全部を選んだときは両端が run の端なので**何も起きない**
 * （全解除は `cutAll` の役目で、ここではない）。
 */
export function cutAroundSelection(
  run: string[],
  cuts: boolean[],
  selectedIds: string[]
): boolean[] {
  const selected = new Set(selectedIds)
  const next = [...cuts]
  let start: number | null = null
  // 番兵として run.length まで回し、末尾で終わる塊も閉じる。
  for (let index = 0; index <= run.length; index += 1) {
    const inside = index < run.length && selected.has(run[index]!)
    if (inside && start === null) start = index
    if (!inside && start !== null) {
      // [start, index-1] が 1 つの塊。両端の外側だけを切る。
      if (start > 0) next[start - 1] = true
      if (index - 1 < run.length - 1) next[index - 1] = true
      start = null
    }
  }
  return next
}

/** 境目をひとつ繋ぐ。範囲外は黙って無視する。 */
export function joinAt(cuts: boolean[], boundaryIndex: number): boolean[] {
  if (boundaryIndex < 0 || boundaryIndex >= cuts.length) return [...cuts]
  const next = [...cuts]
  next[boundaryIndex] = false
  return next
}

/** 全部バラバラにする。`cutAroundSelection` では表せない唯一の操作。 */
export function cutAll(cuts: boolean[]): boolean[] {
  return cuts.map(() => true)
}

/** 表示用。その写真が属するまとまりの枚数。 */
export function blockSizeOf(blocks: Blocks, photoId: string): number {
  return blocks.find(block => block.includes(photoId))?.length ?? 0
}
