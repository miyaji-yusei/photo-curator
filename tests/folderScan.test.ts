import { describe, expect, it } from 'vitest'
import type { SourceEntry, SourceIO } from '~/composables/backends/web/sourceIO'
import { PickerIO, joinPath } from '~/composables/backends/web/sourceIO'
import { isVideoName, scanFolder, skipPairedRaw } from '~/utils/folderScan'

/** サイドカーは走査に関係しない。 */
const noSidecar = {
  sidecarAccess: () => Promise.resolve('none' as const),
  readSidecar: () => Promise.resolve(null),
  writeSidecar: () => Promise.reject(new Error('not used')),
  writeSidecarChecked: () => Promise.reject(new Error('not used'))
}

/** `{ 'a/b.jpg': size, 'a/': 0 }` のような平らな表から作る偽のフォルダ。 */
function fakeFolder(tree: Record<string, string[]>): SourceIO {
  return {
    list: (subPath: string): Promise<SourceEntry[]> => {
      const names = tree[subPath] ?? []
      return Promise.resolve(names.map(name => ({
        name: name.replace(/\/$/, ''),
        isDirectory: name.endsWith('/'),
        size: 10,
        mtimeMs: 1
      })))
    },
    readFile: () => Promise.reject(new Error('not used')),
    ...noSidecar
  }
}

describe('isVideoName', () => {
  it('動画の拡張子を、大文字小文字を問わず見分ける', () => {
    for (const name of ['a.mp4', 'a.MOV', 'a.m4v', 'a.avi', 'a.mts', 'a.M2TS', 'a.3gp', 'a.mkv']) {
      expect(isVideoName(name)).toBe(true)
    }
    for (const name of ['a.jpg', 'a.heic', 'a.dng', 'mp4', 'a.mp4.jpg', 'a']) {
      expect(isVideoName(name)).toBe(false)
    }
  })
})

describe('scanFolder', () => {
  it('サブフォルダまで再帰し、相対パスを鍵にする', async () => {
    const io = fakeFolder({
      '': ['b.jpg', 'sub/', 'a.jpg'],
      sub: ['c.jpg', 'deeper/'],
      'sub/deeper': ['d.png']
    })
    const files = await scanFolder(io)
    expect(files.map(file => file.relativePath)).toEqual([
      'a.jpg', 'b.jpg', 'sub/c.jpg', 'sub/deeper/d.png'
    ])
    expect(files[2]).toMatchObject({ subPath: 'sub', name: 'c.jpg' })
  })

  it('`.` で始まる名前と動画を除く', async () => {
    const io = fakeFolder({
      '': ['.hidden/', '.DS_Store', 'ok.jpg', 'clip.mp4', 'movie.MOV', 'keep/'],
      '.hidden': ['x.jpg'],
      keep: ['.thumb.jpg', 'y.jpg', 'z.mkv']
    })
    const files = await scanFolder(io)
    expect(files.map(file => file.relativePath)).toEqual(['keep/y.jpg', 'ok.jpg'])
  })

  it('打ち切りを頼まれたら、それ以降のフォルダは開かない', async () => {
    let calls = 0
    const io: SourceIO = {
      list: () => { calls += 1; return Promise.resolve([{ name: 'sub', isDirectory: true, size: 0, mtimeMs: 0 }]) },
      readFile: () => Promise.reject(new Error('not used')),
      ...noSidecar
    }
    await scanFolder(io, { isCancelled: () => calls >= 2 })
    expect(calls).toBe(2)
  })
})

describe('PickerIO', () => {
  it('選ばれたファイルを鍵で引ける。無ければ理由つきで失敗する', async () => {
    const io = new PickerIO()
    const file = new File(['x'], 'a.jpg')
    io.add('a.jpg', file)
    expect((await io.list('')).map(entry => entry.name)).toEqual(['a.jpg'])
    expect(await io.readFile('', 'a.jpg')).toBe(file)
    await expect(io.readFile('', 'b.jpg')).rejects.toThrow('リロード')
  })
})

describe('joinPath', () => {
  it('根では名前だけ、それ以外は / でつなぐ', () => {
    expect(joinPath('', 'a.jpg')).toBe('a.jpg')
    expect(joinPath('x/y', 'a.jpg')).toBe('x/y/a.jpg')
  })
})

describe('scanFolder: RAW+JPEG の組（U46）', () => {
  it('同じフォルダに同名の JPEG がある RAW は結果に入れない', async () => {
    const io = fakeFolder({
      '': ['IMG_1.JPG', 'IMG_1.CR2', 'IMG_2.CR2', 'IMG_3.jpeg', 'IMG_3.NEF', 'sub/', 'IMG_4.HEIC', 'img_5.jpg', 'IMG_5.CR2'],
      sub: ['IMG_1.CR2']
    })
    const files = await scanFolder(io)
    expect(files.map(file => file.relativePath).sort()).toEqual([
      'IMG_1.JPG', 'IMG_2.CR2', 'IMG_3.jpeg', 'IMG_4.HEIC', 'img_5.jpg', 'sub/IMG_1.CR2'
    ])
  })

  it('PNG・WebP や HEIC との組は外さない。RAW 同士も外さない', async () => {
    const io = fakeFolder({
      '': ['a.png', 'a.dng', 'b.webp', 'b.arw', 'c.heic', 'c.jpg', 'd.cr2', 'd.nef']
    })
    const files = await scanFolder(io)
    expect(files.map(file => file.name).sort()).toEqual([
      'a.dng', 'a.png', 'b.arw', 'b.webp', 'c.heic', 'c.jpg', 'd.cr2', 'd.nef'
    ])
  })
})

describe('skipPairedRaw', () => {
  it('入力の並びを保つ', () => {
    const make = (relativePath: string) => ({ relativePath, subPath: '', name: relativePath, size: 1, mtimeMs: 1 })
    const kept = skipPairedRaw(['b.cr2', 'a.JPG', 'a.cr3', 'c.cr2'].map(make))
    expect(kept.map(file => file.relativePath)).toEqual(['b.cr2', 'a.JPG', 'c.cr2'])
  })
})

describe('RAW+JPEG の組の設定（U46）', () => {
  it('設定がオフなら全部を対象にする。オンなら組の RAW を外す。省略はオン', async () => {
    const tree = { '': ['a.jpg', 'a.cr2', 'b.cr2'] }
    const names = async (pairRawJpeg?: boolean) =>
      (await scanFolder(fakeFolder(tree), pairRawJpeg === undefined ? {} : { pairRawJpeg })).map(file => file.name).sort()
    expect(await names(false)).toEqual(['a.cr2', 'a.jpg', 'b.cr2'])
    expect(await names(true)).toEqual(['a.jpg', 'b.cr2'])
    expect(await names()).toEqual(['a.jpg', 'b.cr2'])
  })

  it('skipPairedRaw(files, false) は入力をそのまま返す', () => {
    const files = ['a.jpg', 'a.cr2'].map(name => ({ relativePath: name, subPath: '', name }))
    expect(skipPairedRaw(files, false)).toEqual(files)
  })
})
