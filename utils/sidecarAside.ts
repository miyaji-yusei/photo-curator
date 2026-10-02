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

/**
 * 同じ印・同じ秒の退避に付ける番号の始まり（今ある番号の最大 + 1。無ければ 1）。片付けで消えた番号を
 * 使い直すと、新しい退避が番号の小さい（古い）扱いになって、すぐ片付けられてしまうため。
 */
export function nextAsideNumber(names: string[], tag: string, stamp: string): number {
  const prefix = `catalog.${tag}.${stamp}`
  let max = 0
  for (const name of names) {
    if (!name.startsWith(prefix) || !name.endsWith('.json')) continue
    const rest = name.slice(prefix.length, -'.json'.length)
    const n = rest === '' ? 1 : /^-\d+$/.test(rest) ? Number(rest.slice(1)) : 0
    if (n > max) max = n
  }
  return max + 1
}

// ---------------------------------------------------------------------------
// 写真の鍵の食い違い（U52 D15）
// ---------------------------------------------------------------------------

/**
 * 取り込む記録（端末の形の鍵）の鍵を、Unicode の正規化（NFC/NFD）だけが違う**端末の行の名前**に合わせる。
 *
 * core は鍵を NFC にそろえて NAS に書き、端末の形へ戻すときも NFC のまま（core は変えない）。端末の行の名前が
 * NFD（Mac で作った名前など）だと、取り込んだ星が行に当たらず 0 になり、セッションの鍵も行と食い違う。
 * 置き換えるのは、端末の行の名前を NFC にしたものと**完全に一致する**オブジェクトの鍵と文字列だけ。
 * NFC と違う名前の行が無ければ、何もせずにそのまま返す。
 */
export function relocalizeKeys<T>(value: T, localPaths: string[]): T {
  const map = new Map<string, string>()
  for (const path of localPaths) {
    const nfc = path.normalize('NFC')
    if (nfc !== path) map.set(nfc, path)
  }
  if (!map.size) return value
  const walk = (node: unknown): unknown => {
    if (typeof node === 'string') return map.get(node) ?? node
    if (Array.isArray(node)) return node.map(walk)
    if (node && typeof node === 'object') {
      const out: Record<string, unknown> = {}
      for (const [key, inner] of Object.entries(node as Record<string, unknown>)) out[map.get(key) ?? key] = walk(inner)
      return out
    }
    return node
  }
  return walk(value) as T
}

/**
 * 古い Android の記録（keyBase なし。共有の根からの相対）の頭を外すための「選んだフォルダのパス」を、
 * 鍵の頭と**大文字小文字・正規化だけが違う**ときに鍵の綴りへ合わせる（U52 D15）。
 *
 * core の推定（`sidecarNormalizeKeys`）は頭とパスの末尾を大文字小文字を区別して比べる。Windows のパスは
 * 大文字小文字を区別しないので、`l:\\photos\\2021`（`\` 区切り）と `Photos/2021/…` は同じフォルダ。合わせないと頭を外せず、
 * 一致率が半分を下回って、ずっと取り込めない。合わなければパスをそのまま返す。
 */
export function alignFolderHint(folderPath: string, keys: string[]): string {
  let common: string[] | null = null
  for (const key of keys) {
    const parts = key.replace(/\\/g, '/').split('/')
    parts.pop()
    if (common === null) {
      common = parts
    } else {
      let length = 0
      while (length < common.length && length < parts.length && common[length] === parts[length]) length++
      common = common.slice(0, length)
    }
  }
  if (!common?.length) return folderPath
  const hint = folderPath.split(/[\\/]/).filter(part => part.length > 0)
  const fold = (text: string) => text.normalize('NFC').toLowerCase()
  for (let length = Math.min(common.length, hint.length); length >= 1; length--) {
    const head = common.slice(0, length)
    const tail = hint.slice(hint.length - length)
    if (head.every((part, index) => fold(part) === fold(tail[index]!))) {
      return [...hint.slice(0, hint.length - length), ...head].join('\\')
    }
  }
  return folderPath
}
