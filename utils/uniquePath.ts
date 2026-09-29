/**
 * 写真の鍵（`relativePath`）を一意にする。
 *
 * core の Session・サイドカーは `relativePath` を鍵にするので、同じ鍵が 2 枚あると
 * 星が混ざる。ピッカーは**フォルダ名を持たない**ため、別々のアルバムから
 * 同じ名前（`IMG_0001.JPG`）を選ぶと簡単に重なる。
 *
 * 重なったら、2 枚目以降を `name (2).jpg`・`name (3).jpg` のように拡張子の前へ
 * 番号を足して逃がす。最初の 1 枚は名前のまま（既存の行の鍵を変えないため）。
 */
export function uniquePaths(wanted: string[], taken: Iterable<string> = []): string[] {
  const used = new Set<string>(taken)
  return wanted.map(path => {
    let candidate = path
    for (let number = 2; used.has(candidate); number += 1) {
      candidate = withNumber(path, number)
    }
    used.add(candidate)
    return candidate
  })
}

/** `a/b.jpg` → `a/b (2).jpg`。拡張子が無ければ末尾に足す。先頭の `.` は拡張子とみなさない。 */
export function withNumber(path: string, number: number): string {
  const slash = path.lastIndexOf('/')
  const dot = path.lastIndexOf('.')
  const cut = dot > slash + 1 ? dot : path.length
  return `${path.slice(0, cut)} (${number})${path.slice(cut)}`
}
