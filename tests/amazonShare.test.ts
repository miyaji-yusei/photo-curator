import { describe, expect, it } from 'vitest'
import {
  keyFor, parseContentDate, parseKey, parseShareUrl, readShare, viewBoxUrl, type AmazonNode
} from '~/lib/amazonShare'

describe('parseShareUrl', () => {
  it('photos 形式から host と shareId を取り出す', () => {
    expect(parseShareUrl('https://www.amazon.co.jp/photos/share/abcDEF-12_3')).toEqual({ host: 'www.amazon.co.jp', shareId: 'abcDEF-12_3' })
  })
  it('clouddrive 形式も読める', () => {
    expect(parseShareUrl('https://www.amazon.com/clouddrive/share/xyz789')).toEqual({ host: 'www.amazon.com', shareId: 'xyz789' })
  })
  it('前後の空白と後ろのクエリは無視する', () => {
    expect(parseShareUrl('  https://www.amazon.co.jp/photos/share/abc?ref=x  ')).toEqual({ host: 'www.amazon.co.jp', shareId: 'abc' })
  })
  it('Amazon でないリンクは拒否する', () => {
    expect(parseShareUrl('https://example.com/photos/share/abc')).toBeNull()
  })
})

describe('key', () => {
  it('行き来できる（PC と同じ形）', () => {
    const key = keyFor({ host: 'www.amazon.co.jp', shareId: 'abc' })
    expect(key).toBe('www.amazon.co.jp|abc')
    expect(parseKey(key)).toEqual({ host: 'www.amazon.co.jp', shareId: 'abc' })
  })
  it('区切りが無ければ null', () => {
    expect(parseKey('abc')).toBeNull()
  })
})

describe('parseContentDate', () => {
  it('末尾の Z を無視してそのままの数字で読む', () => {
    expect(parseContentDate('2021-07-23T13:13:29.000Z')).toBe(Date.UTC(2021, 6, 23, 13, 13, 29))
  })
  it('読めなければ null', () => {
    expect(parseContentDate(undefined)).toBeNull()
    expect(parseContentDate('yesterday')).toBeNull()
  })
})

describe('viewBoxUrl', () => {
  it('長辺を両方に入れる', () => {
    expect(viewBoxUrl('https://cdn/templink/x', 160)).toBe('https://cdn/templink/x?viewBox=160,160')
  })
})

function file(id: string, contentType: string, tempLink = `https://cdn/${id}`): AmazonNode {
  return { id, name: `${id}.jpg`, kind: 'FILE', contentProperties: { contentType }, tempLink }
}

function fakeFetcher(tree: Record<string, AmazonNode[]>, rootName = '2026-09-24 01:23') {
  const calls: string[] = []
  const fetcher = async (url: string) => {
    calls.push(url)
    if (url.includes('/drive/v1/shares/')) {
      return new Response(JSON.stringify({ nodeInfo: { id: 'root', name: rootName } }))
    }
    const nodeId = url.match(/nodes\/([^/]+)\/children/)![1]!
    const offset = Number(url.match(/offset=(\d+)/)![1])
    const all = tree[nodeId] ?? []
    return new Response(JSON.stringify({ count: all.length, data: all.slice(offset, offset + 200) }))
  }
  return { fetcher, calls }
}

describe('readShare', () => {
  it('アルバムが 1 つならその名前。image/* の FILE だけ集める', async () => {
    const { fetcher } = fakeFetcher({
      root: [{ id: 'album', name: 'ツウィ', kind: 'VISUAL_COLLECTION' }],
      album: [file('a', 'image/jpeg'), file('v', 'video/mp4'), file('b', 'image/png')]
    })
    const share = await readShare({ host: 'www.amazon.co.jp', shareId: 's' }, fetcher)
    expect(share.name).toBe('ツウィ')
    expect(share.key).toBe('www.amazon.co.jp|s')
    expect(share.photos.map(p => p.id)).toEqual(['a', 'b'])
  })

  it('直下が FILE なら共有の名前を使う', async () => {
    const { fetcher } = fakeFetcher({ root: [file('a', 'image/jpeg')] })
    const share = await readShare({ host: 'www.amazon.co.jp', shareId: 's' }, fetcher)
    expect(share.name).toBe('2026-09-24 01:23')
  })

  it('200 件を超えると offset で送る', async () => {
    const many = Array.from({ length: 450 }, (_, i) => file(`p${i}`, 'image/jpeg'))
    const { fetcher, calls } = fakeFetcher({ root: many })
    const share = await readShare({ host: 'www.amazon.co.jp', shareId: 's' }, fetcher)
    expect(share.photos).toHaveLength(450)
    expect(calls.filter(c => c.includes('/children')).map(c => c.match(/offset=(\d+)/)![1])).toEqual(['0', '200', '400'])
  })

  it('FILE 以外は 2 階層まで潜る', async () => {
    const { fetcher } = fakeFetcher({
      root: [{ id: 'l1', name: 'l1', kind: 'FOLDER' }],
      l1: [{ id: 'l2', name: 'l2', kind: 'FOLDER' }, file('a', 'image/jpeg')],
      l2: [{ id: 'l3', name: 'l3', kind: 'FOLDER' }, file('b', 'image/jpeg')],
      l3: [file('c', 'image/jpeg')]
    })
    const share = await readShare({ host: 'www.amazon.co.jp', shareId: 's' }, fetcher)
    expect(share.photos.map(p => p.id)).toEqual(['b', 'a'])
  })

  it('404 は「削除されたか、無効」と言う', async () => {
    const fetcher = async () => new Response('{"message":"ShareId does not exists"}', { status: 404 })
    await expect(readShare({ host: 'www.amazon.co.jp', shareId: 'x' }, fetcher)).rejects.toThrow('このリンクは削除されたか、無効です。')
  })

  it('網が無ければそう言う', async () => {
    const fetcher = async () => { throw new TypeError('Failed to fetch') }
    await expect(readShare({ host: 'www.amazon.co.jp', shareId: 'x' }, fetcher)).rejects.toThrow('ネットワークにつながっていません。')
  })
})
