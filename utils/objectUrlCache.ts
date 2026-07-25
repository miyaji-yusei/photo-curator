/**
 * Blob から作った URL の使い回し。
 *
 * 5,000 枚の一覧で `createObjectURL` を作りっぱなしにすると、Blob が
 * 解放されずメモリを食い続ける。**上限つきの LRU** にして、古いものから
 * `revokeObjectURL` する。上限まで達しない普通の使い方では 1 枚 1 回しか作らない。
 */
export class ObjectUrlCache {
  private readonly urls = new Map<string, string>()

  constructor(private readonly limit = 2_000) {}

  /** 既にあればそれを返し、無ければ作る。 */
  get(key: string, blob: Blob): string {
    const existing = this.urls.get(key)
    if (existing) {
      // 参照されたものを新しい側へ動かす（LRU の順序更新）。
      this.urls.delete(key)
      this.urls.set(key, existing)
      return existing
    }
    const url = URL.createObjectURL(blob)
    this.urls.set(key, url)
    this.evictIfNeeded()
    return url
  }

  peek(key: string): string | undefined {
    return this.urls.get(key)
  }

  release(key: string) {
    const url = this.urls.get(key)
    if (!url) return
    URL.revokeObjectURL(url)
    this.urls.delete(key)
  }

  /** プロジェクトを切り替えるときなど、まとめて捨てる。 */
  releaseAll() {
    for (const url of this.urls.values()) URL.revokeObjectURL(url)
    this.urls.clear()
  }

  get size() {
    return this.urls.size
  }

  private evictIfNeeded() {
    while (this.urls.size > this.limit) {
      const oldest = this.urls.keys().next()
      if (oldest.done) return
      this.release(oldest.value)
    }
  }
}
