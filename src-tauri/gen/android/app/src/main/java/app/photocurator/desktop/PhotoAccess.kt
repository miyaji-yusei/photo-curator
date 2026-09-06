package app.photocurator.desktop

import android.content.ContentUris
import android.content.Context
import android.net.Uri
import android.os.Build
import android.provider.MediaStore
import android.util.Log
import org.json.JSONArray
import org.json.JSONObject
import java.io.InputStream

/**
 * 端末の写真を Rust へ渡す口。**Rust から JNI で静的メソッドとして呼ぶ。**
 *
 * Tauri のプラグイン機構ではなく直接 JNI にしているのは、必要なのが
 * 「一覧を返す」「バイト列を返す」の 2 つだけで、Activity のやりとりが
 * 要らないため。権限の要求だけは Activity が要るので `MainActivity` が持つ。
 *
 * **MediaStore を使い、SAF のフォルダ選択は使わない。** アプリ自身が
 * アルバム（bucket）の一覧を出せるので、システムのピッカーを挟む必要がない。
 * 挟むと Activity Result を Rust まで運ぶ仕掛けが要る。
 *
 * 返すのはすべて JSON 文字列。JNI で構造体を組むより、境界が 1 本で済む。
 */
object PhotoAccess {
    /** このアプリが扱う形式。デスクトップの `is_supported` と揃えてある。 */
    private val MIME_TYPES = arrayOf("image/jpeg", "image/png", "image/webp")

    @Volatile
    private var context: Context? = null

    /** `MainActivity` から一度だけ渡してもらう。 */
    @JvmStatic
    fun attach(context: Context) {
        this.context = context.applicationContext
    }

    private fun requireContext(): Context =
        context ?: throw IllegalStateException("PhotoAccess にコンテキストが渡されていません。")

    private fun collection(): Uri =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL)
        } else {
            MediaStore.Images.Media.EXTERNAL_CONTENT_URI
        }

    private fun mimeSelection(): Pair<String, Array<String>> {
        val placeholders = MIME_TYPES.joinToString(",") { "?" }
        return "${MediaStore.Images.Media.MIME_TYPE} IN ($placeholders)" to MIME_TYPES
    }

    /**
     * 写真の入っているアルバム（bucket）の一覧。
     * **これがデスクトップの「フォルダを選ぶ」に相当する。**
     */
    @JvmStatic
    fun listAlbums(): String {
        val (where, args) = mimeSelection()
        val projection = arrayOf(
            MediaStore.Images.Media.BUCKET_ID,
            MediaStore.Images.Media.BUCKET_DISPLAY_NAME
        )
        // bucket ごとの枚数は SQL の GROUP BY が使えないので、数えながら畳む。
        val counts = LinkedHashMap<String, Pair<String, Int>>()
        requireContext().contentResolver.query(
            collection(), projection, where, args,
            "${MediaStore.Images.Media.DATE_TAKEN} DESC"
        )?.use { cursor ->
            val idColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media.BUCKET_ID)
            val nameColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media.BUCKET_DISPLAY_NAME)
            while (cursor.moveToNext()) {
                val id = cursor.getString(idColumn) ?: continue
                val name = cursor.getString(nameColumn) ?: id
                val current = counts[id]
                counts[id] = name to ((current?.second ?: 0) + 1)
            }
        }
        val array = JSONArray()
        for ((id, entry) in counts) {
            array.put(
                JSONObject()
                    .put("id", id)
                    .put("name", entry.first)
                    .put("count", entry.second)
            )
        }
        return array.toString()
    }

    /**
     * あるアルバムの写真。**撮影時刻の昇順**で返す。
     * デスクトップの `run_scan` が撮影順に並べるのと揃えてある。
     *
     * `bucketId` が空なら端末の全写真。
     */
    @JvmStatic
    fun listPhotos(bucketId: String): String {
        val (mimeWhere, mimeArgs) = mimeSelection()
        val where = if (bucketId.isEmpty()) mimeWhere
        else "$mimeWhere AND ${MediaStore.Images.Media.BUCKET_ID} = ?"
        val args = if (bucketId.isEmpty()) mimeArgs else mimeArgs + bucketId

        val projection = arrayOf(
            MediaStore.Images.Media._ID,
            MediaStore.Images.Media.DISPLAY_NAME,
            MediaStore.Images.Media.RELATIVE_PATH,
            MediaStore.Images.Media.SIZE,
            MediaStore.Images.Media.DATE_MODIFIED
        )
        val array = JSONArray()
        requireContext().contentResolver.query(
            collection(), projection, where, args,
            "${MediaStore.Images.Media.DATE_TAKEN} ASC"
        )?.use { cursor ->
            val idColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media._ID)
            val nameColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media.DISPLAY_NAME)
            val pathColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media.RELATIVE_PATH)
            val sizeColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media.SIZE)
            val modifiedColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media.DATE_MODIFIED)
            while (cursor.moveToNext()) {
                val id = cursor.getLong(idColumn)
                val name = cursor.getString(nameColumn) ?: "photo.jpg"
                array.put(
                    JSONObject()
                        .put("uri", ContentUris.withAppendedId(collection(), id).toString())
                        .put("name", name)
                        .put("relativePath", (cursor.getString(pathColumn) ?: "") + name)
                        .put("size", cursor.getLong(sizeColumn))
                        // MediaStore は秒。**ミリ秒に直す。**
                        // ここを間違えると fingerprint が毎回ずれて再解析が走り続ける。
                        .put("modifiedAt", cursor.getLong(modifiedColumn) * 1000L)
                )
            }
        }
        return array.toString()
    }

    /** 1 枚ぶんの mtime と size。fingerprint に使う。無ければ null。 */
    @JvmStatic
    fun statPhoto(uri: String): String? {
        val projection = arrayOf(
            MediaStore.Images.Media.SIZE,
            MediaStore.Images.Media.DATE_MODIFIED
        )
        requireContext().contentResolver.query(Uri.parse(uri), projection, null, null, null)
            ?.use { cursor ->
                if (!cursor.moveToFirst()) return null
                return JSONObject()
                    .put("size", cursor.getLong(0))
                    .put("modifiedAt", cursor.getLong(1) * 1000L)
                    .toString()
            }
        return null
    }

    /**
     * 写真のバイト列。`length` が 0 以下なら最後まで。
     *
     * **先頭だけ読めることが要点。** EXIF 埋め込みサムネイル経路は先頭 26KB
     * 程度で済む（実測）。ここを常に全体にすると、1 枚 6.7MB を毎回運ぶことになる。
     */
    @JvmStatic
    fun readBytes(uri: String, offset: Long, length: Int): ByteArray? {
        return try {
            requireContext().contentResolver.openInputStream(Uri.parse(uri))?.use { stream ->
                skipExactly(stream, offset)
                if (length <= 0) stream.readBytes() else readAtMost(stream, length)
            }
        } catch (error: Exception) {
            // 消された・権限が無い・壊れている。1 枚で全体を止めない。
            // **理由は必ず残す。** 握り潰すと実機で原因に辿り着けない。
            Log.w("PhotoAccess", "readBytes(" + uri + ") に失敗した。", error)
            null
        }
    }

    /** `skip` は要求より少なく進むことがあるので、届くまで繰り返す。 */
    private fun skipExactly(stream: InputStream, offset: Long) {
        var remaining = offset
        while (remaining > 0) {
            val skipped = stream.skip(remaining)
            if (skipped <= 0) return
            remaining -= skipped
        }
    }

    private fun readAtMost(stream: InputStream, length: Int): ByteArray {
        val buffer = ByteArray(length)
        var filled = 0
        while (filled < length) {
            val read = stream.read(buffer, filled, length - filled)
            if (read < 0) break
            filled += read
        }
        return if (filled == length) buffer else buffer.copyOf(filled)
    }
}
