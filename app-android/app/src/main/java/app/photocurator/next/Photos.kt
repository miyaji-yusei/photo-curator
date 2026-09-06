package app.photocurator.next

import android.content.ContentUris
import android.content.Context
import android.graphics.Bitmap
import android.os.Build
import android.provider.MediaStore
import android.util.Log
import android.util.Size
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * 端末の写真を読む。**Tauri 版と違い、ここが最短距離。**
 *
 * 以前は Kotlin で読んだものを JSON にして JNI で Rust へ渡し、Rust から
 * WebView へ返していた。いまは Compose が直接この結果を使う。
 */

data class Album(
    val id: String,
    val name: String,
    val count: Int,
    /** 一覧の見出しに出す 1 枚。 */
    val coverId: Long
)

data class Photo(
    val id: Long,
    val name: String,
    /** フォルダ内の相対パス。**共有するときの鍵。** */
    val relativePath: String,
    val size: Long,
    val takenAt: Long
) {
    /** この端末での指し先。共有しない。 */
    val uri get() = ContentUris.withAppendedId(COLLECTION, id)
}

private val COLLECTION = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
    MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL)
} else {
    MediaStore.Images.Media.EXTERNAL_CONTENT_URI
}

/** このアプリが扱う形式。 */
private val MIME_TYPES = arrayOf("image/jpeg", "image/png", "image/webp")

private fun mimeSelection(): Pair<String, Array<String>> {
    val placeholders = MIME_TYPES.joinToString(",") { "?" }
    return "${MediaStore.Images.Media.MIME_TYPE} IN ($placeholders)" to MIME_TYPES
}

object Photos {
    private const val TAG = "Photos"

    /** アルバム（bucket）の一覧。枚数と、見出しに使う 1 枚を添える。 */
    suspend fun albums(context: Context): List<Album> = withContext(Dispatchers.IO) {
        val (where, args) = mimeSelection()
        val projection = arrayOf(
            MediaStore.Images.Media._ID,
            MediaStore.Images.Media.BUCKET_ID,
            MediaStore.Images.Media.BUCKET_DISPLAY_NAME
        )
        // bucket ごとの枚数は SQL の GROUP BY が使えないので、数えながら畳む。
        val found = LinkedHashMap<String, Album>()
        context.contentResolver.query(
            COLLECTION, projection, where, args,
            "${MediaStore.Images.Media.DATE_TAKEN} DESC"
        )?.use { cursor ->
            val idColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media._ID)
            val bucketColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media.BUCKET_ID)
            val nameColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media.BUCKET_DISPLAY_NAME)
            while (cursor.moveToNext()) {
                val bucket = cursor.getString(bucketColumn) ?: continue
                val name = cursor.getString(nameColumn) ?: bucket
                val photoId = cursor.getLong(idColumn)
                val prior = found[bucket]
                found[bucket] = if (prior == null) {
                    Album(bucket, name, 1, photoId)
                } else {
                    prior.copy(count = prior.count + 1)
                }
            }
        }
        found.values.sortedByDescending { it.count }
    }

    /** あるアルバムの写真。**撮影時刻の昇順**で返す。 */
    suspend fun photos(context: Context, albumId: String): List<Photo> = withContext(Dispatchers.IO) {
        val (mimeWhere, mimeArgs) = mimeSelection()
        val where = "$mimeWhere AND ${MediaStore.Images.Media.BUCKET_ID} = ?"
        val args = mimeArgs + albumId
        val projection = arrayOf(
            MediaStore.Images.Media._ID,
            MediaStore.Images.Media.DISPLAY_NAME,
            MediaStore.Images.Media.RELATIVE_PATH,
            MediaStore.Images.Media.SIZE,
            MediaStore.Images.Media.DATE_TAKEN,
            MediaStore.Images.Media.DATE_MODIFIED
        )
        val out = ArrayList<Photo>()
        context.contentResolver.query(
            COLLECTION, projection, where, args,
            "${MediaStore.Images.Media.DATE_TAKEN} ASC"
        )?.use { cursor ->
            val idColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media._ID)
            val nameColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media.DISPLAY_NAME)
            val pathColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media.RELATIVE_PATH)
            val sizeColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media.SIZE)
            val takenColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media.DATE_TAKEN)
            val modifiedColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media.DATE_MODIFIED)
            while (cursor.moveToNext()) {
                val name = cursor.getString(nameColumn) ?: "photo.jpg"
                // DATE_TAKEN が無い写真がある（スキャンした画像など）。
                // **無いものを 0 にすると全部が同じ時刻になる**ので mtime に落とす。
                val taken = cursor.getLong(takenColumn).takeIf { it > 0 }
                    ?: (cursor.getLong(modifiedColumn) * 1000L)
                out += Photo(
                    id = cursor.getLong(idColumn),
                    name = name,
                    relativePath = (cursor.getString(pathColumn) ?: "") + name,
                    size = cursor.getLong(sizeColumn),
                    takenAt = taken
                )
            }
        }
        out
    }

    /**
     * OS が持っているサムネイル。**自分で作らない。**
     *
     * `loadThumbnail` は MediaStore 側の縮小画像を返す（無ければ作る）。
     * Tauri 版は原本を読んで自分で縮小していたが、それは 1 枚 6.7MB を
     * 読むということで、2,000 枚では 13GB になる。ここが native の一番の利点。
     */
    fun thumbnail(context: Context, photo: Photo, edge: Int = 512): Bitmap? = try {
        context.contentResolver.loadThumbnail(photo.uri, Size(edge, edge), null)
    } catch (error: Exception) {
        // 消された・壊れている。1 枚で全体を止めない。**理由は必ず残す。**
        Log.w(TAG, "サムネイルを読めない: ${photo.name}", error)
        null
    }

    /**
     * `loadThumbnail` の速さを測る。**「自分で作る」をやめられるかの判断材料。**
     * 結果は logcat に出す。
     */
    suspend fun measureThumbnails(
        context: Context,
        photos: List<Photo>,
        edge: Int
    ): String = withContext(Dispatchers.IO) {
        val sample = photos.take(30)
        if (sample.isEmpty()) return@withContext "写真がありません"
        var ok = 0
        var bytes = 0L
        val started = System.nanoTime()
        for (photo in sample) {
            val bitmap = thumbnail(context, photo, edge)
            if (bitmap != null) {
                ok += 1
                bytes += bitmap.byteCount.toLong()
                bitmap.recycle()
            }
        }
        val elapsedMs = (System.nanoTime() - started) / 1_000_000.0
        val each = elapsedMs / sample.size
        val line = "loadThumbnail(${edge}px): ${sample.size} 枚を ${"%.0f".format(elapsedMs)}ms" +
            "（1 枚 ${"%.1f".format(each)}ms、成功 $ok、平均 ${bytes / maxOf(1, ok) / 1024}KB）"
        Log.i(TAG, line)
        line
    }
}
