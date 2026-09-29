/**
 * サムネイル・表示用画像の実体（Blob）と、その object URL。
 *
 * **行（`store.ts`）とは別のストアに置く。** 一覧のために行を読むだけのとき、
 * 画像の実体まで引きずらないため。Swift の殻ではここをファイル（Caches）に差し替える。
 */
import { STORE_DISPLAYS, STORE_THUMBNAILS, deleteOne, getOne, putOne, withStores } from '~/utils/browserStore'
import { ObjectUrlCache } from '~/utils/objectUrlCache'

export interface PhotoImages {
  thumbnail?: Blob | null
  display?: Blob | null
}

export interface PhotoUrls {
  thumbnailUrl: string | null
  displayUrl: string | null
}

export interface BlobStore {
  /** 渡したものだけ書く（null・省略は書かない）。 */
  put: (photoId: string, images: PhotoImages) => Promise<void>
  /** 写真ごとの URL。保存された画像が無ければ null。 */
  load: (photoIds: string[]) => Promise<Map<string, PhotoUrls>>
  /** セッション中だけ手元にある原本（ピッカー）の URL。 */
  originalUrl: (photoId: string, file: File) => string
  remove: (photoIds: string[]) => Promise<void>
}

interface BlobRow { photoId: string, blob: Blob }

export function createIdbBlobStore(): BlobStore {
  const thumbnailUrls = new ObjectUrlCache()
  const originalUrls = new ObjectUrlCache(64)
  /** 表示用は 1 枚 87KB 前後。選別中に見る範囲だけを持てば足りる。 */
  const displayUrls = new ObjectUrlCache(64)

  return {
    put: async (photoId, images) => {
      if (!images.thumbnail && !images.display) return
      await withStores([STORE_THUMBNAILS, STORE_DISPLAYS], 'readwrite', async transaction => {
        if (images.thumbnail) await putOne(transaction, STORE_THUMBNAILS, { photoId, blob: images.thumbnail })
        // **原本はリロードで失われることがある。** 表示用を残しておかないと、
        // 次に開いたときの選別画面が 256px に落ちる。
        if (images.display) await putOne(transaction, STORE_DISPLAYS, { photoId, blob: images.display })
        // 作り直したときに古い URL を使い回さない。
        if (images.thumbnail) thumbnailUrls.release(photoId)
        if (images.display) displayUrls.release(photoId)
      })
    },

    load: async photoIds => {
      const blobs = await withStores([STORE_THUMBNAILS, STORE_DISPLAYS], 'readonly', async transaction => {
        const result = new Map<string, { thumb?: Blob, display?: Blob }>()
        for (const id of photoIds) {
          const thumb = await getOne<BlobRow>(transaction, STORE_THUMBNAILS, id)
          const display = await getOne<BlobRow>(transaction, STORE_DISPLAYS, id)
          result.set(id, { thumb: thumb?.blob, display: display?.blob })
        }
        return result
      })
      const urls = new Map<string, PhotoUrls>()
      for (const [id, found] of blobs) {
        urls.set(id, {
          thumbnailUrl: found.thumb ? thumbnailUrls.get(id, found.thumb) : null,
          displayUrl: found.display ? displayUrls.get(id, found.display) : null
        })
      }
      return urls
    },

    originalUrl: (photoId, file) => originalUrls.get(photoId, file),

    remove: async photoIds => {
      if (!photoIds.length) return
      await withStores([STORE_THUMBNAILS, STORE_DISPLAYS], 'readwrite', async transaction => {
        for (const id of photoIds) {
          await deleteOne(transaction, STORE_THUMBNAILS, id)
          await deleteOne(transaction, STORE_DISPLAYS, id)
        }
      })
      for (const id of photoIds) {
        thumbnailUrls.release(id)
        displayUrls.release(id)
        originalUrls.release(id)
      }
    }
  }
}
