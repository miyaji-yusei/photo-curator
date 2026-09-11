package app.photocurator.next

import android.content.Context
import android.util.Log
import androidx.exifinterface.media.ExifInterface
import coil.ImageLoader
import coil.decode.DataSource
import coil.decode.ImageSource
import coil.fetch.DrawableResult
import coil.fetch.FetchResult
import coil.fetch.Fetcher
import coil.fetch.SourceResult
import coil.request.Options
import okio.Buffer
import java.io.ByteArrayInputStream

/**
 * どの大きさの絵が欲しいか。**役割を混ぜないために型で分ける。**
 *
 * 前は boolean 1 つで「原本かどうか」しか言えず、選別画面が 160x120 の
 * サムネイルを引き伸ばして出していた。3 つに分けて、取り違えを型で防ぐ。
 */
enum class SmbSize {
    /** EXIF の縮小画像（160x120）。小さく並べるところだけ。 */
    Thumb,

    /** 表示用画像（長辺 1024/1536）。**選別・連写判定はこれ。** */
    Display,

    /** 原本。拡大表示だけ。 */
    Full
}

/** NAS の写真 1 枚を指すもの。**Coil に渡す形。** */
data class SmbImage(
    val nasId: String,
    val path: String,
    val size: SmbSize,
    /** Display のときの長辺。鍵に含めるので、設定を変えれば別物になる。 */
    val edge: Int = 1024
)

/** EXIF から取れたもの。**向きも一緒に取る。** */
data class SmbExif(
    val takenAt: Long?,
    val thumbnail: ByteArray?,
    val orientation: Int = ExifInterface.ORIENTATION_NORMAL
)

object SmbExifReader {
    private const val TAG = "SmbExif"

    /**
     * 先頭これだけ読めば EXIF は入っている。JPEG の APP1 は先頭にあり、
     * 縮小画像もその中に入っている。
     *
     * **網越しではここがそのまま待ち時間になる。** 128KB では 187 枚で
     * 90 秒かかった。64KB でも取れなかった例は出ていない。
     */
    const val HEAD_BYTES = 64 * 1024

    fun parse(head: ByteArray, fallbackAt: Long): SmbExif = try {
        val exif = ExifInterface(ByteArrayInputStream(head))
        val taken = exif.getAttribute(ExifInterface.TAG_DATETIME_ORIGINAL)
            ?: exif.getAttribute(ExifInterface.TAG_DATETIME)
        SmbExif(
            takenAt = taken?.let { parseExifTime(it) } ?: fallbackAt,
            thumbnail = exif.thumbnailBytes,
            orientation = exif.getAttributeInt(
                ExifInterface.TAG_ORIENTATION, ExifInterface.ORIENTATION_NORMAL
            )
        )
    } catch (error: Exception) {
        // 読めなくても止めない。**時刻は更新時刻に落とす。**
        Log.w(TAG, "EXIF を読めなかった", error)
        SmbExif(fallbackAt, null)
    }

    /** EXIF の向きを絵に当てる。**回っていないものは触らない。** */
    fun applyOrientation(
        bitmap: android.graphics.Bitmap,
        orientation: Int
    ): android.graphics.Bitmap {
        val matrix = android.graphics.Matrix()
        when (orientation) {
            ExifInterface.ORIENTATION_ROTATE_90 -> matrix.postRotate(90f)
            ExifInterface.ORIENTATION_ROTATE_180 -> matrix.postRotate(180f)
            ExifInterface.ORIENTATION_ROTATE_270 -> matrix.postRotate(270f)
            ExifInterface.ORIENTATION_FLIP_HORIZONTAL -> matrix.postScale(-1f, 1f)
            ExifInterface.ORIENTATION_FLIP_VERTICAL -> matrix.postScale(1f, -1f)
            else -> return bitmap
        }
        return android.graphics.Bitmap.createBitmap(
            bitmap, 0, 0, bitmap.width, bitmap.height, matrix, true
        )
    }

    private fun parseExifTime(value: String): Long? = try {
        java.text.SimpleDateFormat("yyyy:MM:dd HH:mm:ss", java.util.Locale.JAPAN)
            .parse(value)?.time
    } catch (error: Exception) {
        null
    }
}

/**
 * Coil に NAS の写真を読ませる。
 *
 * **置いてあるものから順に見る。** 網へ行くのは、どこにも無いときだけ。
 */
class SmbFetcher(
    private val context: Context,
    private val image: SmbImage
) : Fetcher {

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

    /** 一覧用。EXIF の縮小画像。 */
    private suspend fun thumb(): FetchResult? {
        ThumbCache.read(context, image.nasId, image.path)?.let {
            return bytesResult(it, DataSource.DISK)
        }
        val nas = nas() ?: return null
        val password = password(nas) ?: return null
        val head = (Smb.head(nas, password, image.path, SmbExifReader.HEAD_BYTES)
            as? SmbResult.Ok)?.value ?: return null
        val exif = SmbExifReader.parse(head, 0L)
        val bytes = exif.thumbnail ?: return display()
        val decoded = android.graphics.BitmapFactory
            .decodeByteArray(bytes, 0, bytes.size) ?: return null
        val turned = SmbExifReader.applyOrientation(decoded, exif.orientation)
        ThumbCache.write(context, image.nasId, image.path, turned)
        return DrawableResult(
            drawable = android.graphics.drawable.BitmapDrawable(context.resources, turned),
            isSampled = true,
            dataSource = DataSource.NETWORK
        )
    }

    /**
     * 選別用。**準備で作ってあるはずのもの。**
     *
     * まだ無ければその場で作る（原本を読む）。それも無理なら、
     * せめてサムネイルを出す。**何も出さないよりは粗くても出す。**
     */
    private suspend fun display(): FetchResult? {
        Renders.read(context, image.nasId, image.path, image.edge)?.let {
            return bytesResult(it, DataSource.DISK)
        }
        val nas = nas()
        val password = nas?.let { password(it) }
        if (nas != null && password != null) {
            val whole = (Smb.whole(nas, password, image.path) as? SmbResult.Ok)?.value
            if (whole != null) {
                val orientation = SmbExifReader.parse(whole, 0L).orientation
                val made = Renders.write(
                    context, image.nasId, image.path, image.edge, whole, orientation
                )
                if (made) {
                    Renders.read(context, image.nasId, image.path, image.edge)?.let {
                        return bytesResult(it, DataSource.NETWORK)
                    }
                }
                return bytesResult(whole, DataSource.NETWORK)
            }
        }
        // 網に行けない。**粗くても出す。** 選別は続けられる。
        ThumbCache.read(context, image.nasId, image.path)?.let {
            return bytesResult(it, DataSource.DISK)
        }
        return null
    }

    /** 拡大用。原本。**置かない。** 1 枚 6MB を溜めても使い道がない。 */
    private suspend fun full(): FetchResult? {
        val nas = nas() ?: return null
        val password = password(nas) ?: return null
        val whole = (Smb.whole(nas, password, image.path) as? SmbResult.Ok)?.value
            ?: return display()
        return bytesResult(whole, DataSource.NETWORK)
    }

    private suspend fun nas(): Nas? =
        NasStore.all(context).firstOrNull { it.id == image.nasId }

    private suspend fun password(nas: Nas): String? = Session.password(context, nas)

    class Factory(private val context: Context) : Fetcher.Factory<SmbImage> {
        override fun create(data: SmbImage, options: Options, imageLoader: ImageLoader) =
            SmbFetcher(context, data)
    }
}

/**
 * 同じ写真を同じものだと分からせる。
 *
 * **大きさまで含めて鍵にする。** 含めないと、一覧用の 160x120 を
 * 選別画面に出してしまう（実際それが起きた）。
 */
class SmbKeyer : coil.key.Keyer<SmbImage> {
    override fun key(data: SmbImage, options: coil.request.Options): String =
        "smb:${data.nasId}:${data.path}:${data.size}:${data.edge}"
}

/**
 * このアプリの画像読み込み。**NAS の写真もここを通す。**
 */
object Images {
    @Volatile
    private var loader: ImageLoader? = null

    fun loader(context: Context): ImageLoader = loader ?: synchronized(this) {
        loader ?: ImageLoader.Builder(context.applicationContext)
            .components {
                add(SmbFetcher.Factory(context.applicationContext))
                add(AmazonFetcher.Factory(context.applicationContext))
                // **鍵が無いと Coil は同じ写真だと分からない。**
                // 分からなければ覚えられず、毎回読み直すことになる。
                add(SmbKeyer())
                add(AmazonKeyer())
            }
            .memoryCache {
                coil.memory.MemoryCache.Builder(context.applicationContext)
                    .maxSizePercent(0.25)
                    .build()
            }
            .diskCache {
                coil.disk.DiskCache.Builder()
                    .directory(context.applicationContext.cacheDir.resolve("images"))
                    .maxSizeBytes(256L * 1024 * 1024)
                    .build()
            }
            .respectCacheHeaders(false)
            .build()
            .also { loader = it }
    }
}
