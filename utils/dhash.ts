/**
 * dHash（difference hash）をブラウザ側で計算する。
 *
 * デスクトップ（Rust）の `d_hash_of` と**同じ意味**の値を出す:
 * 9×8 に縮めて灰色にし、横に隣り合う画素の明暗を比べて 64bit にする。
 * ビットの並びも `y * 8 + x` で揃えてあるので、16 進表記も同じ形になる。
 *
 * 縮小の補間だけは Rust の `Triangle` とブラウザの実装で完全一致しない。
 * ただし連写のまとめ閾値は**プロジェクトごとに学習**するので、
 * 数ビットのずれは学習側が吸収する。iPad のライブラリは PC と独立で、
 * ハッシュを突き合わせることも無い。
 */

/** 比較のために 1 列多く取る。隣との差を見るので幅は高さ+1。 */
export const D_HASH_WIDTH = 9
export const D_HASH_HEIGHT = 8

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

/**
 * 輝度の並びから 64bit の dHash を作り、16 桁の 16 進で返す。
 *
 * JS のビット演算は 32bit で桁が溢れるため BigInt で組む。
 * 1 枚あたり 64 回なので、5,000 枚でも実行時間には影響しない。
 */
export function dHashFromLuma(
  luma: ArrayLike<number>,
  width = D_HASH_WIDTH,
  height = D_HASH_HEIGHT
): string {
  let bits = 0n
  const columns = width - 1
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < columns; x += 1) {
      const left = luma[y * width + x] ?? 0
      const right = luma[y * width + x + 1] ?? 0
      // 左が明るいときだけ立てる。Rust 側と同じ向き。
      if (left > right) bits |= 1n << BigInt(y * columns + x)
    }
  }
  return bits.toString(16).padStart(16, '0')
}

/** 9×8 の RGBA から dHash を出す。ワーカーが使う入口。 */
export function dHashFromRgba(rgba: ArrayLike<number>): string {
  return dHashFromLuma(lumaFromRgba(rgba, D_HASH_WIDTH * D_HASH_HEIGHT))
}

/** 2 つの dHash のハミング距離。連写判定の材料。 */
export function hammingDistance(left: string, right: string): number {
  let value = BigInt(`0x${left}`) ^ BigInt(`0x${right}`)
  let count = 0
  while (value > 0n) {
    if (value & 1n) count += 1
    value >>= 1n
  }
  return count
}
