import { describe, expect, it } from 'vitest'
import { previewStateOf } from '~/utils/previewState'
import type { Photo } from '~/types/photo'

const photo = (over: Partial<Photo>): Photo => ({
  id: 'a', projectId: 'p', path: 'a.jpg', relativePath: 'a.jpg', name: 'a.jpg',
  capturedAt: 1, dHash: null, rating: 0, thumbnailPath: null, displayPath: null,
  ...over
})

describe('空のタイルの意味', () => {
  /**
   * **何も無いタイルが並ぶだけでは、利用者は何を待てばいいか分からない。**
   * 作っている途中なのか、順番待ちなのか、読めなかったのか、そもそも
   * 対応していない形式なのかで、できることが違う。
   */
  it('サムネイルがあれば出すだけ', () => {
    expect(previewStateOf(photo({ thumbnailPath: '/t/a.jpg' }), false)).toBe('ready')
  })

  it('理由が無く、解析が走っていれば「作成中」', () => {
    expect(previewStateOf(photo({}), true)).toBe('generating')
  })

  it('理由が無く、解析が止まっていれば「待機」', () => {
    expect(previewStateOf(photo({}), false)).toBe('queued')
  })

  it('読めなかったものは「読めません」', () => {
    const failed = photo({ analysisError: 'ファイルを開けませんでした（移動・削除・権限）。' })
    expect(previewStateOf(failed, true)).toBe('failed')
  })

  /** 非対応は直しようがないので、壊れているのとは別に言う。 */
  it('非対応の形式は別に扱う', () => {
    const heic = photo({ analysisError: '画像を読み取れませんでした（破損または非対応の形式）。' })
    expect(previewStateOf(heic, false)).toBe('unsupported')
  })

  /** サムネイルがあれば、あとから付いた理由より現物を優先する。 */
  it('サムネイルがあれば理由より優先する', () => {
    const both = photo({ thumbnailPath: '/t/a.jpg', analysisError: '読めません' })
    expect(previewStateOf(both, false)).toBe('ready')
  })
})
