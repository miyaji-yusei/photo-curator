/**
 * Amazon などの URL からバイトを取る口。
 *
 * 環境ごとに取り方が違う（中継サーバー・Chrome 拡張の直接 fetch・Swift の `URLSession`）ので、
 * `createLocalBackend` に差し込めるようにしてある。
 *
 * **表示は別**。画像 CDN は CORS が無く `fetch` では読めないが、crossOrigin を付けない素の
 * `<img src>` なら出せる。ここが要るのは、バイトが要るとき（指紋・ZIP）だけ。
 */
import { fetchAmazonBytes, viewBoxUrl } from '~/lib/amazonShare'

export interface Fetcher {
  /**
   * tempLink のバイト。`edge` があれば長辺をその大きさに縮めたもの。
   * 取れなければ null（例外は投げない）。
   */
  fetchBytes: (tempLink: string, edge?: number) => Promise<Blob | null>
}

/** 何も取れない。テスト用。 */
export const noFetcher: Fetcher = {
  fetchBytes: () => Promise.resolve(null)
}

/** 既定。自分のサーバー（`/api/amazon/image`）を経由する。静的配信には無いので、失敗は null。 */
export const relayFetcher: Fetcher = {
  fetchBytes: (tempLink, edge) => fetchAmazonBytes(tempLink, edge)
}

/**
 * tempLink をそのまま `fetch` する。**いまはどこからも使わない**。
 * CORS を越えられる環境（Chrome 拡張など）で `relayFetcher` の代わりに差し込む。
 */
export const directFetcher: Fetcher = {
  fetchBytes: async (tempLink, edge) => {
    try {
      const response = await fetch(edge === undefined ? tempLink : viewBoxUrl(tempLink, edge))
      return response.ok ? await response.blob() : null
    } catch {
      return null
    }
  }
}
