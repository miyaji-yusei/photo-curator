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

export function amazonCsv(rows: AmazonCsvRow[]): string {
  const lines = ['relative_path,rating,captured_at,name']
  for (const row of rows) {
    lines.push([
      csvCell(row.relativePath), String(row.rating), row.capturedAt === null ? '' : String(row.capturedAt), csvCell(row.name)
    ].join(','))
  }
  return `${lines.join('\n')}\n`
}
