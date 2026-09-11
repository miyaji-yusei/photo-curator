package app.photocurator.next

import android.content.Context
import coil.ImageLoader
import coil.decode.DataSource
import coil.decode.ImageSource
import coil.fetch.FetchResult
import coil.fetch.Fetcher
import coil.fetch.SourceResult
import coil.request.Options
import okio.Buffer

/**
 * Amazon の写真の絵。**大きさまで含めて 1 つの値にする**（NAS と同じ作り）。
 *
 * サムネイルと表示用画像は Amazon に縮小させて端末に置く。拡大は原本を
 * そのたびに取り、**端末には置かない**（設計 08 章 6）。
 */
data class AmazonImage(val ref: AmazonRef, val size: SmbSize, val edge: Int = 0)

class AmazonFetcher(
    private val context: Context,
    private val image: AmazonImage
) : Fetcher {

    private val cacheId = Amazon.linkOf(image.ref.shareKey).cacheId
    private val nodeId = image.ref.nodeId

    private fun bytesResult(bytes: ByteArray, source: DataSource) = SourceResult(
        source = ImageSource(Buffer().apply { write(bytes) }, context),
        mimeType = null,
        dataSource = source
    )

    override suspend fun fetch(): FetchResult? = when (image.size) {
        SmbSize.Thumb -> thumb()
        SmbSize.Display -> display()
        SmbSize.Full -> full()
    }

    /** 一覧用。**一度取ったら二度と取りに行かない。** */
    private suspend fun thumb(): FetchResult? {
        ThumbCache.read(context, cacheId, nodeId)?.let { return bytesResult(it, DataSource.DISK) }
        val bytes = (Amazon.image(image.ref, Amazon.THUMB) as? SmbResult.Ok)?.value ?: return null
        ThumbCache.put(context, cacheId, nodeId, bytes)
        return bytesResult(bytes, DataSource.NETWORK)
    }

    /**
     * 選別用。**準備で作ってあるはずのもの。** 無ければその場で頼んで置く。
     * 網に行けなければ、せめてサムネイルを出す（何も出さないよりは粗くても出す）。
     */
    private suspend fun display(): FetchResult? {
        Renders.read(context, cacheId, nodeId, image.edge)?.let {
            return bytesResult(it, DataSource.DISK)
        }
        val got = Amazon.image(image.ref, image.edge)
        if (got is SmbResult.Ok) {
            Renders.put(context, cacheId, nodeId, image.edge, got.value)
            return bytesResult(got.value, DataSource.NETWORK)
        }
        ThumbCache.read(context, cacheId, nodeId)?.let { return bytesResult(it, DataSource.DISK) }
        return null
    }

    /**
     * 拡大用。**原本をそのたびに取る。置かない。**
     * 取れなければ（網が無い・リンクが消えた）表示用画像に戻す。
     */
    private suspend fun full(): FetchResult? {
        val got = Amazon.image(image.ref, null)
        if (got is SmbResult.Ok) return bytesResult(got.value, DataSource.NETWORK)
        return display()
    }

    class Factory(private val context: Context) : Fetcher.Factory<AmazonImage> {
        override fun create(data: AmazonImage, options: Options, imageLoader: ImageLoader) =
            AmazonFetcher(context, data)
    }
}

/**
 * 同じ写真を同じものだと分からせる。**tempLink は鍵に入れない**（変わりうるので）。
 * 大きさまで含める（SmbKeyer と同じ教訓）。
 */
class AmazonKeyer : coil.key.Keyer<AmazonImage> {
    override fun key(data: AmazonImage, options: Options): String =
        "amz:${data.ref.shareKey}:${data.ref.nodeId}:${data.size}:${data.edge}"
}
