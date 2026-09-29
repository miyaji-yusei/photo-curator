// Amazon Photos の画像を中継する（設計 08 章の Web 版フル機能・別セッション）。
//
// Web からは画像 CDN が CORS 非対応でバイトを読めない（lib/amazonShare.ts 冒頭）。
// このサーバー（Nitro）はブラウザではないので CORS を受けない。ここで代わりに
// 取得し、自分のオリジンから返せば、ブラウザの fetch・canvas から普通に読める
// （指紋作り・ZIP化に使う）。
//
// **自分は静的サイトとしても配信する**（GitHub Pages。scripts/build-pages.mjs）。
// そちらではこの経路自体が存在しない。呼ぶ側（lib/backends/amazonWeb.ts）は
// 失敗を「読めなかった 1 枚」として飲み込み、無ければ無いなりに動く
// （表示（<img> 直読み）は元々このルートを経由しないので影響しない）。
//
// **任意の URL は中継しない。** tempLink のホストは Amazon の画像 CDN
// （*.drive.amazonaws.com）だけに絞る。無制限にすると誰でも使えるオープン
// プロキシになってしまう。

const ALLOWED_HOST = /^content-[a-z0-9-]+\.drive\.amazonaws\.com$/
const MAX_EDGE = 8000

export default defineEventHandler(async (event) => {
  const query = getQuery(event)
  const tempLink = typeof query.tempLink === 'string' ? query.tempLink : ''

  // **HTTP のステータス行（statusMessage）に日本語を乗せない**（h3 の警告どおり、
  // 将来サニタイズされる・一部のクライアントで壊れる）。理由は message（本文）へ。
  let target: URL
  try {
    target = new URL(tempLink)
  } catch {
    throw createError({ statusCode: 400, message: 'tempLink が URL の形をしていません。' })
  }
  if (target.protocol !== 'https:' || !ALLOWED_HOST.test(target.hostname)) {
    throw createError({ statusCode: 400, message: 'Amazon の画像 CDN 以外は中継しません。' })
  }

  if (query.edge !== undefined) {
    const edge = Number(query.edge)
    if (!Number.isInteger(edge) || edge <= 0 || edge > MAX_EDGE) {
      throw createError({ statusCode: 400, message: 'edge の値が正しくありません。' })
    }
    target.searchParams.set('viewBox', `${edge},${edge}`)
  }

  const response = await fetch(target.toString())
  if (!response.ok || !response.body) {
    throw createError({ statusCode: 502, message: `Amazon から読めませんでした（${response.status}）。` })
  }
  setResponseHeader(event, 'Content-Type', response.headers.get('content-type') ?? 'image/jpeg')
  // 中身は変わらない（tempLink + 大きさで一意）。ブラウザ内で使い回してよい。
  setResponseHeader(event, 'Cache-Control', 'private, max-age=86400')
  return response.body
})
