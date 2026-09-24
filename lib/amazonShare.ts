// Amazon Photos 共有リンクの読み取り（設計 08 章）。Web 用。PC は src-tauri/src/amazon.rs。
//
// **ログイン・Cookie は使わない。読み取り専用。** JSON の API（① 共有の中身・② 子の一覧）は
// CORS 開放なのでブラウザから直接読める。画像 CDN（tempLink の先）は CORS 非対応で、
// バイトとしては読めない（fetch も crossOrigin 付きの <img> も拒否。2026-09-24 実測）。
// 読めるのは「crossorigin を付けない素の <img src>」での表示だけなので、ここでは
// **tempLink の URL を組み立てるところまで**しかしない。

import { civilTimestampMs } from '~/utils/captureTime'

export interface AmazonShareSource {
  host: string
  shareId: string
}

export interface AmazonNode {
  id: string
  name: string
  kind: string
  contentProperties?: {
    contentType?: string
    contentDate?: string
    size?: number
  }
  tempLink?: string
}

/** FILE 以外は 2 階層まで潜る（08章「落とし穴」3）。 */
const MAX_DEPTH = 2
/** 1 回に取れる件数の上限。超えると 400（08章 2.4）。 */
const PAGE_LIMIT = 200

export type Fetcher = (url: string) => Promise<Response>

/** `https://www.amazon.co.jp/photos/share/{id}` と `.../clouddrive/share/{id}` の両方を受ける。 */
export function parseShareUrl(url: string): AmazonShareSource | null {
  const match = url.trim().match(/^(?:https?:\/\/)?(www\.amazon\.[a-z.]+)\/(?:photos|clouddrive)\/share\/([A-Za-z0-9_-]+)/)
  if (!match) return null
  return { host: match[1]!, shareId: match[2]! }
}

/** `ProjectSource.key` の形（08章 5.1: `"{host}|{shareId}"`）。PC と同じ。 */
export function keyFor(source: AmazonShareSource): string {
  return `${source.host}|${source.shareId}`
}

export function parseKey(key: string): AmazonShareSource | null {
  const at = key.indexOf('|')
  if (at <= 0 || at === key.length - 1) return null
  return { host: key.slice(0, at), shareId: key.slice(at + 1) }
}

/** 撮影時刻（ミリ秒）。**末尾の `Z` を信じない**（08章「落とし穴」6。amazon.rs と同じ読み方）。 */
export function parseContentDate(contentDate: string | undefined): number | null {
  if (!contentDate) return null
  const match = contentDate.match(/^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})/)
  if (!match) return null
  const [, y, mo, d, h, mi, s] = match.map(Number) as [number, number, number, number, number, number, number]
  return civilTimestampMs(y, mo, d, h, mi, s)
}

/** 縮小版の URL（08章 2.2 ③）。原本より大きい値を頼んでも原本の大きさで来る。 */
export function viewBoxUrl(tempLink: string, edge: number): string {
  return `${tempLink}?viewBox=${edge},${edge}`
}

export function isImageNode(node: AmazonNode): boolean {
  return node.kind === 'FILE' && (node.contentProperties?.contentType ?? '').startsWith('image/')
}

async function getJson<T>(url: string, fetcher: Fetcher): Promise<T> {
  let response: Response
  try {
    response = await fetcher(url)
  } catch {
    throw new Error('ネットワークにつながっていません。')
  }
  if (response.status === 404) throw new Error('このリンクは削除されたか、無効です。')
  if (!response.ok) throw new Error(`Amazon から読めませんでした（${response.status}）。`)
  return response.json() as Promise<T>
}

/** ① 共有の中身。 */
export async function fetchShareRoot(source: AmazonShareSource, fetcher: Fetcher = fetch): Promise<{ nodeId: string; name: string }> {
  const url = `https://${source.host}/drive/v1/shares/${source.shareId}?shareId=${source.shareId}&resourceVersion=V2&ContentType=JSON`
  const info = await getJson<{ nodeInfo: { id: string; name: string } }>(url, fetcher)
  return { nodeId: info.nodeInfo.id, name: info.nodeInfo.name }
}

/** ② 子の一覧。200 件ずつ offset で送る（nextToken は来ない。08章 2.2）。 */
export async function fetchChildren(source: AmazonShareSource, nodeId: string, fetcher: Fetcher = fetch): Promise<AmazonNode[]> {
  const out: AmazonNode[] = []
  let offset = 0
  for (;;) {
    const url = `https://${source.host}/drive/v1/nodes/${nodeId}/children?asset=ALL&limit=${PAGE_LIMIT}&offset=${offset}&searchOnFamily=false&tempLink=true&shareId=${source.shareId}&sort=%5B%27contentProperties.contentDate+ASC%27%5D&resourceVersion=V2&ContentType=JSON`
    const page = await getJson<{ count: number; data: AmazonNode[] }>(url, fetcher)
    out.push(...page.data)
    offset += page.data.length
    if (page.data.length === 0 || offset >= page.count) break
  }
  return out
}

export interface AmazonShare {
  key: string
  /** 共有の直下がアルバム 1 つだけならそのアルバム名、そうでなければ共有の名前（08章「プロジェクト名の決め方」）。 */
  name: string
  /** `image/*` の FILE だけ（**拡張子で判定しない**。08章「落とし穴」1・2）。撮影時刻の昇順。 */
  photos: AmazonNode[]
}

/** 共有リンク全体を読む。FILE 以外は 2 階層まで潜る。 */
export async function readShare(source: AmazonShareSource, fetcher: Fetcher = fetch): Promise<AmazonShare> {
  const root = await fetchShareRoot(source, fetcher)
  const top = await fetchChildren(source, root.nodeId, fetcher)
  const photos: AmazonNode[] = []
  const collect = async (nodes: AmazonNode[], depth: number) => {
    for (const node of nodes) {
      if (node.kind === 'FILE') {
        if (isImageNode(node)) photos.push(node)
      } else if (depth < MAX_DEPTH) {
        await collect(await fetchChildren(source, node.id, fetcher), depth + 1)
      }
    }
  }
  await collect(top, 0)
  const albums = top.filter(n => n.kind !== 'FILE')
  const name = albums.length === 1 ? albums[0]!.name : root.name
  return { key: keyFor(source), name, photos }
}
