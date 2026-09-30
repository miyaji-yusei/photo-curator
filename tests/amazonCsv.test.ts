import { describe, expect, it } from 'vitest'
import { amazonCsv, csvCell, resultsCsv } from '~/utils/amazonCsv'

describe('amazonCsv', () => {
  it('先頭 3 列は他の出所と同じで、4 列目に name', () => {
    const text = amazonCsv([
      { relativePath: 'node-1', rating: 5, capturedAt: 1_700_000_000_000, name: 'IMG_0001.JPG' },
      { relativePath: 'node-2', rating: 0, capturedAt: null, name: 'IMG_0002.JPG' }
    ])
    expect(text.split('\n')).toEqual([
      'relative_path,rating,captured_at,name',
      'node-1,5,1700000000000,IMG_0001.JPG',
      'node-2,0,,IMG_0002.JPG',
      ''
    ])
  })

  it('カンマ・引用符・改行を含む名前は囲む', () => {
    expect(csvCell('a,b')).toBe('"a,b"')
    expect(csvCell('say "hi"')).toBe('"say ""hi"""')
    expect(csvCell('日本語 名前.jpg')).toBe('日本語 名前.jpg')
    const text = amazonCsv([{ relativePath: 'n', rating: 3, capturedAt: null, name: 'a,"b".jpg' }])
    expect(text.split('\n')[1]).toBe('n,3,,"a,""b"".jpg"')
  })

  it('Amazon 以外は 3 列だけ（name を出さない）', () => {
    const text = resultsCsv([{ relativePath: 'a/b.jpg', rating: 2, capturedAt: 5, name: 'b.jpg' }], false)
    expect(text.split('\n')).toEqual(['relative_path,rating,captured_at', 'a/b.jpg,2,5', ''])
  })
})
