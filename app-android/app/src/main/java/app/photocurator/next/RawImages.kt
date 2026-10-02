package app.photocurator.next

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.drawable.BitmapDrawable
import android.net.Uri
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
import coil.size.Dimension
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okio.Buffer
import java.io.ByteArrayInputStream
import java.io.FileInputStream
import java.nio.ByteBuffer

/**
 * RAW の写真 1 枚を指すもの（U49）。**Coil に渡す形。** 端末なら `local`、NAS なら `smb`。
 *
 * Android の標準のデコーダは RAW を読めないので、RAW の中のプレビュー JPEG を取り出して
 * 絵にする（[RawPreview]）。取り出せなければ失敗にして、タイルは「RAW（表示できません）」。
 */
data class RawImage(
    val local: Uri?,
    val smb: SmbRef?,
    val size: ImageSize,
    /** Display のときの長辺。鍵に含める。 */
    val edge: Int = 1024
)

/** 取り出したプレビュー。`orientation` は EXIF の値。 */
class RawExtracted(val jpeg: ByteArray, val orientation: Int, val takenAt: Long?)

/** RAW の中にプレビューが見つからなかった。**タイルに「表示できません」を出す印。** */
class RawUnreadable(name: String) : Exception("RAW のプレビューを取り出せません: $name")

object RawImages {
    private const val TAG = "RawImages"

    /** サムネイルに使う最小の長辺。EXIF の縮小画像（160x120）で足りる。 */
    const val THUMB_EDGE = 160

    /** サムネイルとして置く大きさ。並べるところは 256px で頼まれる。 */
    const val THUMB_STORE = 320

    /** 拡大のときの上限。**大きすぎる絵をメモリに広げない。** */
    const val FULL_EDGE = 2048

    /**
     * プレビューを取り出す。`thumb` なら小さいもの、そうでなければ一番大きいもの。
     * 見つからなければ null。
     */
    fun extract(source: ByteSource, thumb: Boolean): RawExtracted? {
        val info = RawPreview.inspect(source)
        val region = (if (thumb) RawPreview.forThumb(info, THUMB_EDGE) else RawPreview.largest(info))
            ?: return null
        val jpeg = RawPreview.bytesOf(source, region) ?: return null
        // 向きは RAW の IFD0 が正。無ければ（RAF・CR3 の一部）JPEG 自身の EXIF を見る。
        val orientation = info.orientation ?: try {
            ExifInterface(ByteArrayInputStream(jpeg))
                .getAttributeInt(ExifInterface.TAG_ORIENTATION, ExifInterface.ORIENTATION_NORMAL)
        } catch (error: Exception) {
            ExifInterface.ORIENTATION_NORMAL
        }
        return RawExtracted(jpeg, orientation, info.takenAt?.let(::parseTime))
    }

    /** 端末の RAW から取り出す。**ファイルの要る範囲だけを読む。** */
    fun local(context: Context, uri: Uri, thumb: Boolean): RawExtracted? = try {
        context.contentResolver.openFileDescriptor(uri, "r")?.use { descriptor ->
            FileInputStream(descriptor.fileDescriptor).channel.use { channel ->
                val source = BlockCache(ByteSource { offset, length ->
                    if (offset < 0 || length <= 0) return@ByteSource null
                    val buffer = ByteBuffer.allocate(length)
                    var at = offset
                    while (buffer.hasRemaining()) {
                        val read = channel.read(buffer, at)
                        if (read <= 0) break
                        at += read
                    }
                    if (buffer.position() == 0) null else buffer.array().copyOf(buffer.position())
                })
                extract(source, thumb)
            }
        }
    } catch (error: Exception) {
        Log.w(TAG, "端末の RAW を読めなかった: $uri", error)
        null
    }

    /**
     * JPEG を、長辺が `target` を下回らない範囲で間引いて読み、向きを当てる。
     * 6000x4000 のプレビューをそのまま広げると 96MB になる。
     */
    fun decode(jpeg: ByteArray, orientation: Int, target: Int): Bitmap? {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(jpeg, 0, jpeg.size, bounds)
        val longest = maxOf(bounds.outWidth, bounds.outHeight)
        if (longest <= 0) return null
        var sample = 1
        while (longest / (sample * 2) >= target) sample *= 2
        val decoded = BitmapFactory.decodeByteArray(
            jpeg, 0, jpeg.size, BitmapFactory.Options().apply { inSampleSize = sample }
        ) ?: return null
        val turned = SmbExifReader.applyOrientation(decoded, orientation)
        if (turned !== decoded) decoded.recycle()
        return turned
    }

    /** 小さく縮める（置くサムネイル用）。**大きくはしない。** */
    fun shrink(bitmap: Bitmap, edge: Int): Bitmap {
        val longest = maxOf(bitmap.width, bitmap.height)
        if (longest <= edge) return bitmap
        val scale = edge.toFloat() / longest
        return Bitmap.createScaledBitmap(
            bitmap,
            maxOf(1, (bitmap.width * scale).toInt()),
            maxOf(1, (bitmap.height * scale).toInt()),
            true
        )
    }

    /** EXIF の時刻（端末の時間帯で読む。JPEG の読み方 [SmbExifReader] と同じ）。 */
    fun parseTime(value: String): Long? = try {
        java.text.SimpleDateFormat("yyyy:MM:dd HH:mm:ss", java.util.Locale.JAPAN).parse(value)?.time
    } catch (error: Exception) {
        null
    }

    /**
     * 連写のまとめに使う小さな絵（端末の RAW）。**OS の縮小画像が取れないときの代わり。**
     * 呼んだ側が片付ける。
     */
    fun localThumbBitmap(context: Context, uri: Uri, edge: Int): Bitmap? {
        val got = local(context, uri, thumb = true) ?: return null
        return decode(got.jpeg, got.orientation, edge)
    }
}

/** Coil に RAW を読ませる。**取り出せなければ投げる**（タイルが「表示できません」を出す）。 */
class RawFetcher(
    private val context: Context,
    private val image: RawImage,
    private val options: Options
) : Fetcher {

    private val name: String get() = image.smb?.path ?: image.local.toString()

    /** 頼まれた大きさ（長辺の px）。分からなければ役割ごとの既定。 */
    private fun target(): Int {
        val w = (options.size.width as? Dimension.Pixels)?.px ?: 0
        val h = (options.size.height as? Dimension.Pixels)?.px ?: 0
        val asked = maxOf(w, h)
        val fallback = when (image.size) {
            ImageSize.Thumb -> 256
            ImageSize.Display -> image.edge
            ImageSize.Full -> RawImages.FULL_EDGE
        }
        return (if (asked > 0) asked else fallback).coerceAtMost(RawImages.FULL_EDGE)
    }

    private fun drawable(bitmap: Bitmap, source: DataSource) = DrawableResult(
        drawable = BitmapDrawable(context.resources, bitmap),
        isSampled = true,
        dataSource = source
    )

    private fun bytesResult(bytes: ByteArray, source: DataSource) = SourceResult(
        source = ImageSource(Buffer().apply { write(bytes) }, context),
        mimeType = null,
        dataSource = source
    )

    override suspend fun fetch(): FetchResult = withContext(Dispatchers.IO) {
        val smb = image.smb
        if (smb != null) fromNas(smb) else fromDevice(image.local ?: throw RawUnreadable(name))
    }

    private fun fromDevice(uri: Uri): FetchResult {
        val got = RawImages.local(context, uri, thumb = image.size == ImageSize.Thumb)
            ?: throw RawUnreadable(name)
        val bitmap = RawImages.decode(got.jpeg, got.orientation, target()) ?: throw RawUnreadable(name)
        return drawable(bitmap, DataSource.DISK)
    }

    private suspend fun fromNas(ref: SmbRef): FetchResult = when (image.size) {
        ImageSize.Thumb -> nasThumb(ref)
        ImageSize.Display -> Renders.exclusive(ref.nasId, ref.path, image.edge) { nasDisplay(ref) }
        ImageSize.Full -> {
            val got = nasExtract(ref, thumb = false) ?: throw RawUnreadable(name)
            val bitmap = RawImages.decode(got.jpeg, got.orientation, target()) ?: throw RawUnreadable(name)
            drawable(bitmap, DataSource.NETWORK)
        }
    }

    /** 一覧用。置いてあればそれ。無ければ小さいプレビューを取り出して置く。 */
    private suspend fun nasThumb(ref: SmbRef): FetchResult {
        ThumbCache.read(context, ref.nasId, ref.path)?.let { return bytesResult(it, DataSource.DISK) }
        val got = nasExtract(ref, thumb = true) ?: throw RawUnreadable(name)
        val decoded = RawImages.decode(got.jpeg, got.orientation, 256) ?: throw RawUnreadable(name)
        val small = RawImages.shrink(decoded, RawImages.THUMB_STORE)
        if (small !== decoded) decoded.recycle()
        ThumbCache.write(context, ref.nasId, ref.path, small)
        return drawable(small, DataSource.NETWORK)
    }

    /** 選別用。準備で作ってあればそれ。無ければ大きいプレビューから作って置く。 */
    private suspend fun nasDisplay(ref: SmbRef): FetchResult {
        Renders.read(context, ref.nasId, ref.path, image.edge)?.let { return bytesResult(it, DataSource.DISK) }
        val got = nasExtract(ref, thumb = false)
        if (got != null) {
            if (Renders.write(context, ref.nasId, ref.path, image.edge, got.jpeg, got.orientation)) {
                Renders.read(context, ref.nasId, ref.path, image.edge)?.let {
                    return bytesResult(it, DataSource.NETWORK)
                }
            }
            RawImages.decode(got.jpeg, got.orientation, image.edge)?.let { return drawable(it, DataSource.NETWORK) }
        }
        // 網に行けない・取り出せない。**粗くても出す。**
        ThumbCache.read(context, ref.nasId, ref.path)?.let { return bytesResult(it, DataSource.DISK) }
        throw RawUnreadable(name)
    }

    private suspend fun nasExtract(ref: SmbRef, thumb: Boolean): RawExtracted? {
        val nas = NasStore.all(context).firstOrNull { it.id == ref.nasId } ?: return null
        val password = NasPasswords.password(context, nas) ?: return null
        return (Smb.ranged(nas, password, ref.path) { RawImages.extract(it, thumb) } as? SmbResult.Ok)?.value
    }

    class Factory(private val context: Context) : Fetcher.Factory<RawImage> {
        override fun create(data: RawImage, options: Options, imageLoader: ImageLoader) =
            RawFetcher(context, data, options)
    }
}

/** 同じ RAW を同じものだと分からせる。**大きさまで鍵に含める**（[SmbKeyer] と同じ理由）。 */
class RawKeyer : coil.key.Keyer<RawImage> {
    override fun key(data: RawImage, options: Options): String =
        "raw:${data.smb?.let { "${it.nasId}:${it.path}" } ?: data.local}:${data.size}:${data.edge}"
}
