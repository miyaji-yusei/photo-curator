package app.photocurator.next

import android.content.Context
import android.graphics.Bitmap
import android.util.Log
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.File

/**
 * NAS の写真の縮小画像を、端末に置いておく。
 *
 * **準備のときに一度読んだものを捨てない。** 指紋を作るために先頭 64KB を
 * 読んでいて、その中に EXIF の縮小画像が入っている。捨てると、一覧を出す
 * たびに同じものをもう一度網から取ることになる。
 *
 * 向きを当ててから置く。読むたびに回すのは無駄だし、当て忘れると
 * 縦の写真が横のまま並ぶ。
 */
object ThumbCache {
    private const val TAG = "ThumbCache"

    private fun dir(context: Context) = File(context.filesDir, "thumbs").apply { mkdirs() }

    /** 道筋から決まる名前。**同じ写真なら同じファイル。** */
    private fun name(cacheId: String, path: String): String =
        "${cacheId}_${path.hashCode().toUInt().toString(16)}.jpg"

    fun file(context: Context, cacheId: String, path: String) =
        File(dir(context), name(cacheId, path))

    fun has(context: Context, cacheId: String, path: String): Boolean =
        file(context, cacheId, path).let { it.exists() && it.length() > 0 }

    suspend fun read(context: Context, cacheId: String, path: String): ByteArray? =
        withContext(Dispatchers.IO) {
            val target = file(context, cacheId, path)
            if (!target.exists() || target.length() == 0L) return@withContext null
            try {
                target.readBytes()
            } catch (error: Exception) {
                Log.w(TAG, "縮小画像を読めなかった: $path", error)
                null
            }
        }

    /** 置く。**向きを当てた絵**を JPEG にして書く。 */
    fun write(context: Context, cacheId: String, path: String, bitmap: Bitmap) {
        try {
            File(dir(context), name(cacheId, path)).writeAtomically {
                it.outputStream().use { out -> bitmap.compress(Bitmap.CompressFormat.JPEG, 88, out) }
            }
        } catch (error: Exception) {
            // 置けなくても止めない。**次に一覧を出すとき網から取るだけ。**
            Log.w(TAG, "縮小画像を置けなかった: $path", error)
        }
    }

    /**
     * 受け取った JPEG をそのまま置く。**Amazon は縮小して向きも直して返す**ので、
     * こちらでデコードし直さない（設計 08 章 6）。
     */
    fun put(context: Context, cacheId: String, path: String, bytes: ByteArray) {
        try {
            File(dir(context), name(cacheId, path)).writeAtomically { it.writeBytes(bytes) }
        } catch (error: Exception) {
            Log.w(TAG, "縮小画像を置けなかった: $path", error)
        }
    }

    /** プロジェクトを消したときに片付ける。 */
    suspend fun clear(context: Context, cacheId: String) = withContext(Dispatchers.IO) {
        try {
            dir(context).listFiles { file -> file.name.startsWith("${cacheId}_") }
                ?.forEach { it.delete() }
            Unit
        } catch (error: Exception) {
            Log.w(TAG, "縮小画像を片付けられなかった", error)
        }
    }
}
