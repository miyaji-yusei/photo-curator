/**
 * オフラインで動かすためのサービスワーカー。
 *
 * ビルド時にハッシュの付いた資産名を埋め込む方式は、生成物と手書きの
 * 一覧がずれると静かに壊れる。ここでは**一度読んだものを覚える**方式にして、
 * 一覧を持たない。初回にオンラインで開いたあとは、機内モードでも起動できる。
 *
 * 写真そのものと星は IndexedDB にあり、fetch を通らないのでここでは扱わない。
 */
const CACHE = 'photo-curator-v1'

self.addEventListener('install', () => {
  // 新しい版をすぐ有効にする。古い版が残ってアプリだけ古いことを防ぐ。
  self.skipWaiting()
})

self.addEventListener('activate', event => {
  event.waitUntil((async () => {
    const names = await caches.keys()
    await Promise.all(names.filter(name => name !== CACHE).map(name => caches.delete(name)))
    await self.clients.claim()
  })())
})

/** 画面遷移。まずネットワーク、駄目なら覚えている画面を返す。 */
async function handleNavigation(request) {
  const cache = await caches.open(CACHE)
  try {
    const response = await fetch(request)
    // 取れたぶんは最新として覚え直す。
    if (response.ok) cache.put(request, response.clone())
    return response
  } catch {
    return (await cache.match(request)) ??
      (await cache.match(new URL('./', self.registration.scope).href)) ??
      Response.error()
  }
}

/** 資産。覚えていればすぐ返し、裏で新しいものを取り直す。 */
async function handleAsset(request) {
  const cache = await caches.open(CACHE)
  const cached = await cache.match(request)
  const update = fetch(request)
    .then(response => {
      if (response.ok) cache.put(request, response.clone())
      return response
    })
    .catch(() => null)
  if (cached) return cached
  const fresh = await update
  return fresh ?? Response.error()
}

self.addEventListener('fetch', event => {
  const { request } = event
  if (request.method !== 'GET') return
  const url = new URL(request.url)
  // 自分の配信元だけ扱う。外部への通信には触らない。
  if (url.origin !== self.location.origin) return

  if (request.mode === 'navigate') {
    event.respondWith(handleNavigation(request))
    return
  }
  event.respondWith(handleAsset(request))
})
