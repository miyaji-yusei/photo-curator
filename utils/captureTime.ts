/**
 * 撮影時刻の推定。デスクトップ（Rust）の `read_capture_time` と同じ順序・
 * 同じ検証で組んである。**選別の並びが撮影順**なので、ここの質が選別の質になる。
 *
 * 根拠の強い順に EXIF → ファイル名 → ファイルの更新時刻。
 * ファイル名を更新時刻より優先するのは、書き出しや転送で EXIF が落ちても
 * `20260630_181932` の類は残ることが多く、更新時刻よりはるかに撮影時刻に近いため。
 */

/**
 * 撮影時刻をどこから取ったか。`filesystem_mtime` は撮影時刻ではない
 * （コピーや展開で大量のファイルが同一の値を持つ）ので、連写の根拠としては弱い。
 * 文字列はデスクトップの `TimestampSource::as_str` と一致させてある。
 */
export type TimestampSource =
  | 'exif_original'
  | 'exif_datetime'
  | 'filename_inferred'
  | 'filesystem_mtime'
  | 'unknown'

export interface CaptureTime {
  at: number
  source: TimestampSource
}

/** 連写の根拠としては弱い経路か。 */
export const isWeakSource = (source: TimestampSource) =>
  source === 'filesystem_mtime' || source === 'unknown'

const isLeapYear = (year: number) => year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0)

export function daysInMonth(year: number, month: number): number {
  switch (month) {
    case 1: case 3: case 5: case 7: case 8: case 10: case 12: return 31
    case 4: case 6: case 9: case 11: return 30
    case 2: return isLeapYear(year) ? 29 : 28
    default: return 0
  }
}

/**
 * 暦の日時をミリ秒にする。**数字の並びとして成立していても暦として不正なら採らない**
 * （13 月や 2 月 30 日を「それらしい値」に化けさせない）。
 * オフセットを持たない EXIF は「現地時刻だが地域は不明」なので UTC とみなす。
 * 連写判定は差分しか見ないため、同一フォルダ内の相対関係は壊れない。
 */
export function civilTimestampMs(
  year: number, month: number, day: number,
  hour: number, minute: number, second: number
): number | null {
  if (!Number.isInteger(year) || year < 1900 || year > 2999) return null
  if (month < 1 || month > 12 || day < 1 || day > daysInMonth(year, month)) return null
  // うるう秒で 60 を書く機材があるため秒だけ 60 を許す。
  if (hour < 0 || hour > 23 || minute < 0 || minute > 59 || second < 0 || second > 60) return null
  return Date.UTC(year, month - 1, day, hour, minute, second)
}

/** 連続した数字の並びだけを取り出す。 */
export function digitGroups(value: string): string[] {
  return value.split(/[^0-9]+/).filter(part => part.length > 0)
}

function splitFixed(text: string, widths: number[]): number[] | null {
  let rest = text
  const parts: number[] = []
  for (const width of widths) {
    if (rest.length < width) return null
    parts.push(Number(rest.slice(0, width)))
    rest = rest.slice(width)
  }
  return parts
}

/** 数字グループの形から日時を読む。対応する形は Rust 側と同じ 3 種類。 */
export function timestampFromGroups(groups: string[]): number | null {
  const lengths = groups.map(group => group.length)
  let parts: number[] | null = null
  if (lengths[0] === 14) {
    // 20260630181932
    parts = splitFixed(groups[0]!, [4, 2, 2, 2, 2, 2])
  } else if (lengths[0] === 8 && lengths[1] === 6) {
    // IMG_20260630_181932
    const head = splitFixed(groups[0]!, [4, 2, 2])
    const tail = splitFixed(groups[1]!, [2, 2, 2])
    parts = head && tail ? [...head, ...tail] : null
  } else if (
    lengths.length >= 6 && lengths[0] === 4 &&
    lengths.slice(1, 6).every(length => length === 2)
  ) {
    // 2026-06-30_18-19-32
    parts = groups.slice(0, 6).map(group => Number(group))
  }
  if (!parts || parts.some(part => !Number.isFinite(part))) return null
  return civilTimestampMs(parts[0]!, parts[1]!, parts[2]!, parts[3]!, parts[4]!, parts[5]!)
}

/**
 * ファイル名から撮影時刻らしい並びを読む。
 * `2026-06-30_18-19-32` / `20260630_181932` / `IMG_20260630_181932` に対応する。
 */
export function filenameCaptureTime(fileName: string): number | null {
  const stem = fileName.replace(/\.[^.]*$/, '')
  const groups = digitGroups(stem)
  for (let start = 0; start < groups.length; start += 1) {
    const at = timestampFromGroups(groups.slice(start))
    if (at !== null) return at
  }
  return null
}

/**
 * EXIF → ファイル名 → 更新時刻の順に落としていく。
 * `exif` は EXIF から読めた結果（無ければ null）。
 */
export function resolveCaptureTime(
  exif: CaptureTime | null,
  fileName: string,
  lastModified: number | null
): CaptureTime | null {
  if (exif) return exif
  const fromName = filenameCaptureTime(fileName)
  if (fromName !== null) return { at: fromName, source: 'filename_inferred' }
  if (lastModified !== null && Number.isFinite(lastModified)) {
    return { at: lastModified, source: 'filesystem_mtime' }
  }
  return null
}
