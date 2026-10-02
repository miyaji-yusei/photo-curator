/**
 * サイドカーの退避の名前と数（U52 D4）。**Android の U44（`SidecarSync.kt` の `tag`・`asideName`・`stampOf`・
 * `asidesToDrop`・`ASIDE_KEEP`）と同じ形**にそろえる。PC の Rust は `src-tauri/src/sidecar.rs` に同じ規則を持つ。
 */

/** NAS の退避を、端末の印ごとにいくつまで残すか（U52 D4。Android の U44 と同じ。CON-3 のため小さく）。 */
export const ASIDE_KEEP = 5

/**
 * 退避のファイル名に使う端末の印（Android の `SidecarSync.tag` と同じ: 英数字・`-`・`_` だけを残して 12 文字。
 * 空なら `unknown`）。同じ id から PC・Web・Android で同じ印になる。
 */
export function asideTag(deviceId: string): string {
  return [...deviceId].filter(c => /^[A-Za-z0-9_-]$/.test(c)).slice(0, 12).join('') || 'unknown'
}

/** 退避の名前に入れる時刻（UTC・`yyyyMMddHHmmss`）。名前の順が時刻の順になる。 */
export function asideStamp(at: number): string {
  return new Date(at).toISOString().replace(/[-:T]/g, '').slice(0, 14)
}

/**
 * NAS の退避の名前（U52 D4。Android の U44 と同じ）: `catalog.<印>.<UTC yyyyMMddHHmmss>.json`、
 * 同じ秒に重なったら `-2` 以降。**前の退避を上書きしない**（無いときだけ作る）。
 */
export function asideName(tag: string, stamp: string, n = 1): string {
  return `catalog.${tag}.${stamp}${n > 1 ? `-${n}` : ''}.json`
}

/** 消してよい退避（その印の時刻つきのうち、新しい `keep` 個より古いもの。時刻の無い古い名前は含まない）。 */
export function asidesToDrop(names: string[], tag: string, keep: number = ASIDE_KEEP): string[] {
  const escaped = tag.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  const pattern = new RegExp(`^catalog\\.${escaped}\\.(\\d{14})(?:-(\\d+))?\\.json$`)
  return names
    .map((name) => {
      const match = pattern.exec(name)
      return match ? { name, stamp: match[1]!, n: Number(match[2] ?? 1) } : null
    })
    .filter((entry): entry is { name: string, stamp: string, n: number } => entry !== null)
    .sort((a, b) => (a.stamp === b.stamp ? b.n - a.n : a.stamp < b.stamp ? 1 : -1))
    .slice(keep)
    .map(entry => entry.name)
}
