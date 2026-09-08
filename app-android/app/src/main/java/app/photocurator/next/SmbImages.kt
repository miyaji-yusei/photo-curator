package app.photocurator.next

import android.content.Context
import android.util.Log
import androidx.exifinterface.media.ExifInterface
import coil.ImageLoader
import coil.decode.DataSource
import coil.decode.ImageSource
import coil.fetch.FetchResult
import coil.fetch.Fetcher
import coil.fetch.DrawableResult
import coil.fetch.SourceResult
import coil.request.Options
import okio.Buffer
import java.io.ByteArrayInputStream

/**
 * NAS の写真 1 枚を指すもの。**Coil に渡す形。**
 *
 * `full` が false なら EXIF の縮小画像だけを取りに行く。並べるだけなら
 * それで足り、原本 6MB を網越しに引かずに済む。
 */
data class SmbImage(val nasId: String, val path: String, val full: Boolean)

/** EXIF から取れたもの。**撮影時刻と縮小画像は同じ 1 回の読みで取れる。** */
/**
 * EXIF から取れたもの。**向きも一緒に取る。**
 *
 * EXIF の縮小画像は「回す前」の絵で、向きは親ファイルの EXIF にしかない。
 * 縮小画像のバイト列だけを渡すと、縦の写真が横のまま並ぶ。
 */
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
    fun applyOrientation(bitmap: android.graphics.Bitmap, orientation: Int): android.graphics.Bitmap {
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
 * **縮小は EXIF のものを使う。** 一覧に出すだけなら 160x120 で足りる。
 * 無いときだけ原本を読む。拡大表示（full）は最初から原本を読む。
 */
class SmbFetcher(
    private val context: Context,
    private val image: SmbImage
) : Fetcher {

    override suspend fun fetch(): FetchResult? {
        val nas = NasStore.all(context).firstOrNull { it.id == image.nasId } ?: return null
        val password = Session.password(context, nas) ?: return null

        if (image.full) {
            // 拡大は原本。JPEG の EXIF が付いたままなので、向きは Coil が当てる。
            val bytes = (Smb.whole(nas, password, image.path) as? SmbResult.Ok)?.value
                ?: return null
            return SourceResult(
                source = ImageSource(Buffer().apply { write(bytes) }, context),
                mimeType = null,
                dataSource = DataSource.NETWORK
            )
        }

        val head = (Smb.head(nas, password, image.path, SmbExifReader.HEAD_BYTES)
            as? SmbResult.Ok)?.value ?: return null
        val exif = SmbExifReader.parse(head, 0L)
        val thumbnail = exif.thumbnail
        if (thumbnail == null) {
            // 縮小画像を持たない写真。**そのときだけ原本を読む。**
            val bytes = (Smb.whole(nas, password, image.path) as? SmbResult.Ok)?.value
                ?: return null
            return SourceResult(
                source = ImageSource(Buffer().apply { write(bytes) }, context),
                mimeType = null,
                dataSource = DataSource.NETWORK
            )
        }

        // **向きを当ててから返す。** 縮小画像は回す前の絵で、向きは親の EXIF に
        // しかない。バイト列のまま渡すと、縦の写真が横のまま並ぶ。
        val bitmap = android.graphics.BitmapFactory
            .decodeByteArray(thumbnail, 0, thumbnail.size) ?: return null
        val turned = SmbExifReader.applyOrientation(bitmap, exif.orientation)
        return DrawableResult(
            drawable = android.graphics.drawable.BitmapDrawable(context.resources, turned),
            isSampled = true,
            dataSource = DataSource.NETWORK
        )
    }

    class Factory(private val context: Context) : Fetcher.Factory<SmbImage> {
        override fun create(data: SmbImage, options: Options, imageLoader: ImageLoader) =
            SmbFetcher(context, data)
    }
}

/**
 * このアプリの画像読み込み。**NAS の写真もここを通す。**
 *
 * 端末の写真は Coil の既定でよいが、NAS は自前で取りに行く必要がある。
 * 1 か所にまとめておけば、どの画面も同じ道で読める。
 */
object Images {
    @Volatile
    private var loader: ImageLoader? = null

    fun loader(context: Context): ImageLoader = loader ?: synchronized(this) {
        loader ?: ImageLoader.Builder(context.applicationContext)
            .components { add(SmbFetcher.Factory(context.applicationContext)) }
            // 網越しの読みは高い。**一度取ったものはディスクに置く。**
            .respectCacheHeaders(false)
            .build()
            .also { loader = it }
    }
}
