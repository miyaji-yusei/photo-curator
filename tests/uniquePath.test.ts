import { describe, expect, it } from 'vitest'
import { uniquePaths, withNumber } from '~/utils/uniquePath'

describe('withNumber', () => {
  it('拡張子の前に番号を入れる', () => {
    expect(withNumber('IMG_1.jpg', 2)).toBe('IMG_1 (2).jpg')
    expect(withNumber('a/b/IMG_1.JPG', 3)).toBe('a/b/IMG_1 (3).JPG')
  })

  it('拡張子が無ければ末尾に足す', () => {
    expect(withNumber('README', 2)).toBe('README (2)')
  })

  it('先頭の点と、フォルダ名の点は拡張子とみなさない', () => {
    expect(withNumber('.hidden', 2)).toBe('.hidden (2)')
    expect(withNumber('v1.2/photo', 2)).toBe('v1.2/photo (2)')
  })
})

describe('uniquePaths', () => {
  it('重ならなければそのまま返す', () => {
    expect(uniquePaths(['a.jpg', 'b.jpg'])).toEqual(['a.jpg', 'b.jpg'])
  })

  it('同じ名前は 2 枚目から番号を付ける', () => {
    expect(uniquePaths(['a.jpg', 'a.jpg', 'a.jpg'])).toEqual(['a.jpg', 'a (2).jpg', 'a (3).jpg'])
  })

  it('もともと `a (2).jpg` がある場合とも重ならない', () => {
    const result = uniquePaths(['a.jpg', 'a (2).jpg', 'a.jpg'])
    expect(new Set(result).size).toBe(3)
    expect(result[0]).toBe('a.jpg')
    expect(result[1]).toBe('a (2).jpg')
    expect(result[2]).toBe('a (3).jpg')
  })

  it('すでに保存済みの鍵とも重ならない', () => {
    expect(uniquePaths(['a.jpg', 'b.jpg'], ['a.jpg'])).toEqual(['a (2).jpg', 'b.jpg'])
  })

  it('大文字小文字は別の名前として扱う（ファイルシステムに合わせない）', () => {
    expect(uniquePaths(['a.jpg', 'A.jpg'])).toEqual(['a.jpg', 'A.jpg'])
  })
})
