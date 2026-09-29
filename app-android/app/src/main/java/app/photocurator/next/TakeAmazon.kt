package app.photocurator.next

import android.content.Context
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Amazon の写真を取り出す。**端末のギャラリーへコピーするだけ。**
 *
 * Amazon 側には書かない（書く口はログインが要る）。原本を読むのは、
 * 本人がこの操作を選んだときだけ（設計 08 章 6）。
 */
object TakeAmazon {
    /**
     * ギャラリーに保存する。**1 枚ずつ読んで、1 枚ずつ書く。**
     *
     * 名前が `.cr2` でも中身が JPEG のことがある（実測）。**拡張子は中身から
     * 付け直す**。そうしないとギャラリーが開けないことがある。
     */
    suspend fun saveToGallery(
        context: Context,
        photos: List<Photo>,
        album: String,
        onProgress: (Int, Int) -> Unit
    ): TakeNas.Done = withContext(Dispatchers.IO) {
        var done = 0
        var failed = 0
        var reason: String? = null
        val folder = TakeNas.galleryFolder(album)

        for (photo in photos) {
            val ref = photo.amazon
            if (ref == null) {
                failed += 1
                continue
            }
            when (val got = Amazon.image(ref, null)) {
                is SmbResult.Failed -> {
                    // **リンクが消えていたら、残りも全部だめ。** 1 枚ずつ待たせない。
                    if (got.reason == Amazon.GONE) {
                        return@withContext TakeNas.Done(done, photos.size - done, got.reason)
                    }
                    failed += 1
                    if (reason == null) reason = got.reason
                }
                is SmbResult.Ok -> {
                    val (extension, mime) = kindOf(got.value)
                    val name = photo.name.substringBeforeLast('.', photo.name) + "." + extension
                    if (TakeNas.insertIntoGallery(context, folder, name, mime, got.value)) {
                        done += 1
                    } else {
                        failed += 1
                        if (reason == null) reason = "端末に書き込めませんでした"
                    }
                }
            }
            onProgress(done + failed, photos.size)
        }
        TakeNas.Done(done, failed, reason)
    }

    /** 中身の先頭から形を見る。**分からなければ JPEG として扱う。** */
    private fun kindOf(bytes: ByteArray): Pair<String, String> {
        fun at(index: Int) = if (index < bytes.size) bytes[index].toInt() and 0xff else -1
        fun text(from: Int, length: Int) =
            if (bytes.size >= from + length) String(bytes, from, length, Charsets.US_ASCII) else ""
        return when {
            at(0) == 0x89 && at(1) == 0x50 -> "png" to "image/png"
            text(0, 4) == "RIFF" && text(8, 4) == "WEBP" -> "webp" to "image/webp"
            text(4, 4) == "ftyp" && text(8, 4).startsWith("hei") -> "heic" to "image/heic"
            else -> "jpg" to "image/jpeg"
        }
    }
}
