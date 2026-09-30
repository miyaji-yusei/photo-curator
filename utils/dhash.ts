/**
 * 指紋（dHash）の材料になる輝度を、ブラウザの画素（RGBA）から出す。
 *
 * **指紋そのものは core（`lib/core.ts` の `dHashFromGray`）が作る。**
 * 9×8 への縮小も、隣との比較も、16 進の並びも core の仕事（設計 04 章「縮小も core で」）。
 * ここに残すのは、RGBA を灰色にする部分だけ。PC の Rust と同じ BT.709 の重み。
 */

/**
 * RGBA から輝度を出す。`image` クレートと同じ BT.709 の重みで、
 * 整数除算まで合わせてある（0.2126 / 0.7152 / 0.0722）。
 */
export function lumaFromRgba(rgba: ArrayLike<number>, pixels: number): Uint8Array {
  const luma = new Uint8Array(pixels)
  for (let index = 0; index < pixels; index += 1) {
    const at = index * 4
    const r = rgba[at] ?? 0
    const g = rgba[at + 1] ?? 0
    const b = rgba[at + 2] ?? 0
    luma[index] = Math.floor((2126 * r + 7152 * g + 722 * b) / 10000)
  }
  return luma
}
