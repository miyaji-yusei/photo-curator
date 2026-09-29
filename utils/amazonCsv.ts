/**
 * Amazon の結果の CSV。先頭 3 列（relative_path・rating・captured_at）は他の出所と同じ。
 * `relative_path` が node id で人には読めないので、4 列目に `name`（ファイル名）を足す。
 */
export interface AmazonCsvRow {
  relativePath: string
  rating: number
  capturedAt: number | null
  name: string
}

/** RFC 4180。カンマ・引用符・改行を含むときだけ囲み、引用符は 2 つにする。 */
export function csvCell(value: string): string {
  return /[",\r\n]/.test(value) ? `"${value.replaceAll('"', '""')}"` : value
}

export interface ResultsCsvRow {
  relativePath: string
  rating: number
  capturedAt: number | null
  /** Amazon のときだけ 4 列目に出す。 */
  name?: string
}

/**
 * 結果の CSV（全部の出所）。列は `relative_path,rating,captured_at`。
 * `withName`（Amazon）のときだけ 4 列目に `name` を足す。
 */
export function resultsCsv(rows: ResultsCsvRow[], withName: boolean): string {
  const lines = [withName ? 'relative_path,rating,captured_at,name' : 'relative_path,rating,captured_at']
  for (const row of rows) {
    const cells = [csvCell(row.relativePath), String(row.rating), row.capturedAt === null ? '' : String(row.capturedAt)]
    if (withName) cells.push(csvCell(row.name ?? ''))
    lines.push(cells.join(','))
  }
  return `${lines.join('\n')}\n`
}

export function amazonCsv(rows: AmazonCsvRow[]): string {
  return resultsCsv(rows, true)
}
