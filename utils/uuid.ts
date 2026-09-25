/**
 * `crypto.randomUUID()` は secure context（HTTPS か localhost）でしか使えない。
 * LAN の IP を `http://` で開いたとき（iPad・Android の実機確認。07章参照）は
 * 存在せず、呼ぶと `crypto.randomUUID is not a function` になる。
 *
 * `crypto.getRandomValues()` は secure context を問わず使えるので、それで
 * UUID v4 を組み立てる（プロジェクト・写真の鍵に使うだけで、暗号用途ではない）。
 */
export function randomUUID(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID()
  }
  const bytes = crypto.getRandomValues(new Uint8Array(16))
  bytes[6] = (bytes[6]! & 0x0f) | 0x40 // version 4
  bytes[8] = (bytes[8]! & 0x3f) | 0x80 // variant
  const hex = Array.from(bytes, b => b.toString(16).padStart(2, '0')).join('')
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
}
