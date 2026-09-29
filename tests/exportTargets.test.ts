import { describe, expect, it } from 'vitest'
import { expandExportTargets, exportTargetsForStars, type ExportRow } from '~/utils/exportTargets'

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

describe('exportTargetsForStars', () => {
  const photo = (relativePath: string, rating: number) => ({ relativePath, rating, capturedAt: null })

  it('選んだ星の写真だけ、大きい星から順に出る', () => {
    const photos = [photo('a', 1), photo('b', 3), photo('c', 3), photo('d', 0)]
    expect(exportTargetsForStars(photos, {}, [1, 3]).map(t => t.relativePath)).toEqual(['b', 'c', 'a'])
  })

  it('連写の仲間は、その 1 枚自身の星が選んだ星に合うものだけ出る', () => {
    // 中身を選別して b・c が ★2、a は据え置きの ★1。
    const photos = [photo('a', 1), photo('b', 2), photo('c', 2), photo('x', 2)]
    const members = { a: ['a', 'b', 'c'] }
    expect(exportTargetsForStars(photos, members, [2]).map(t => t.relativePath)).toEqual(['b', 'c', 'x'])
    expect(exportTargetsForStars(photos, members, [1]).map(t => t.relativePath)).toEqual(['a'])
    expect(exportTargetsForStars(photos, members, [2, 1]).map(t => t.relativePath)).toEqual(['b', 'c', 'x', 'a'])
  })

  it('星が空なら何も出ない・同じ星を重ねても二重に出ない', () => {
    const photos = [photo('a', 2)]
    expect(exportTargetsForStars(photos, {}, [])).toEqual([])
    expect(exportTargetsForStars(photos, {}, [2, 2])).toHaveLength(1)
  })
})
