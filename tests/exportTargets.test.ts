import { describe, expect, it } from 'vitest'
import { expandExportTargets, type ExportRow } from '~/utils/exportTargets'

const at = (path: string) => (path === 'a' ? 100 : null)

describe('expandExportTargets', () => {
  it('単独の写真はそのまま 1 枚', () => {
    const rows: ExportRow[] = [{ relativePath: 'a', rating: 2, mates: ['a'] }]
    expect(expandExportTargets(rows, { a: 2 }, at)).toEqual([{ relativePath: 'a', rating: 2, capturedAt: 100 }])
  })

  it('中身を選別していない連写は、仲間も全員出る', () => {
    const rows: ExportRow[] = [{ relativePath: 'a', rating: 1, mates: ['a', 'b', 'c'] }]
    const out = expandExportTargets(rows, { a: 1, b: 1, c: 1 }, at)
    expect(out.map(t => t.relativePath)).toEqual(['a', 'b', 'c'])
  })

  it('中身を選別した連写は、選んだもの（星が上がったもの）だけ出る', () => {
    // b・c を選んで ★1 → ★2。a は据え置きの ★1。
    const rows: ExportRow[] = [{ relativePath: 'b', rating: 2, mates: ['a', 'b', 'c'] }]
    const out = expandExportTargets(rows, { a: 1, b: 2, c: 2 }, at)
    expect(out.map(t => t.relativePath)).toEqual(['b', 'c'])
    expect(out.every(t => t.rating === 2)).toBe(true)
  })

  it('選別の結果が全員なら、また全員出る', () => {
    const rows: ExportRow[] = [{ relativePath: 'a', rating: 2, mates: ['a', 'b'] }]
    expect(expandExportTargets(rows, { a: 2, b: 2 }, at)).toHaveLength(2)
  })

  it('星の記録が無い写真は ★0 として扱う', () => {
    const rows: ExportRow[] = [{ relativePath: 'a', rating: 0, mates: ['a', 'b'] }]
    expect(expandExportTargets(rows, {}, at)).toHaveLength(2)
  })

  it('同じ写真を二重に出さない', () => {
    const rows: ExportRow[] = [
      { relativePath: 'a', rating: 1, mates: ['a', 'b'] },
      { relativePath: 'b', rating: 1, mates: ['b'] }
    ]
    expect(expandExportTargets(rows, { a: 1, b: 1 }, at).map(t => t.relativePath)).toEqual(['a', 'b'])
  })

  it('撮影時刻を引き継ぐ', () => {
    const rows: ExportRow[] = [{ relativePath: 'a', rating: 1, mates: ['a', 'x'] }]
    const out = expandExportTargets(rows, { a: 1, x: 1 }, at)
    expect(out.find(t => t.relativePath === 'a')?.capturedAt).toBe(100)
    expect(out.find(t => t.relativePath === 'x')?.capturedAt).toBeNull()
  })
})
