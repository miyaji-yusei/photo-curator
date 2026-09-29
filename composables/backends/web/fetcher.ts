/**
 * Amazon などの URL からバイトを取る口。**口だけ**で、中身は T10。
 *
 * 環境ごとに取り方が違う（中継サーバー・Chrome 拡張の直接 fetch・Swift の `URLSession`）ので、
 * `createLocalBackend` に差し込めるようにしてある。
 */
export interface Fetcher {
  /** 取れなければ null（例外は投げない）。 */
  fetchBytes: (url: string) => Promise<Blob | null>
}

/** 既定。まだ何も取れない。 */
export const noFetcher: Fetcher = {
  fetchBytes: () => Promise.resolve(null)
}
