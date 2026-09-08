package app.photocurator.next

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.util.Log
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.File

/**
 * 表示用画像。**原本を一度だけ読んで、選別に使える大きさで残す。**
 *
 * 絵は 3 段階ある。役割を混ぜないことがこの設計の要点:
 *
 * | 段 | 何 | 大きさ | 使う場所 |
 * | --- | --- | --- | --- |
 * | サムネイル | EXIF の縮小画像 | 160x120・約 10KB | 詳細の一覧、まとまりの確認 |
 * | 表示用画像 | 原本を縮小したもの | 長辺 1024/1536・約 150KB | 選別・連写判定・拡大の最初 |
 * | 原本 | そのまま | 6MB 前後 | 拡大表示だけ |
 *
 * NAS の原本は 1 枚 6MB を網越しに読む。選別のたびにこれを引くと待ち時間に
 * ならない。**一度だけ読んで縮小したものを端末に置く**のが表示用画像で、
 * 以後はここからしか読まない。
 *
 * 端末のアルバムには作らない。手元のファイルは Coil が要求した大きさで
 * デコードするので、置いても二重に持つだけになる。
 */
object Renders {
    private const val TAG = "Renders"

    private fun dir(context: Context) = File(context.filesDir, "renders").apply { mkdirs() }

    /**
     * 名前に**大きさを含める。** 設定を変えたときに、古い小さい絵を
     * そのまま出してしまわないため。
     */
    private fun name(nasId: String, path: String, edge: Int): String =
        "${nasId}_${path.hashCode().toUInt().toString(16)}_$edge.jpg"

    fun file(context: Context, nasId: String, path: String, edge: Int) =
        File(dir(context), name(nasId, path, edge))

    fun has(context: Context, nasId: String, path: String, edge: Int): Boolean =
        file(context, nasId, path, edge).let { it.exists() && it.length() > 0 }

    suspend fun read(context: Context, nasId: String, path: String, edge: Int): ByteArray? =
        withContext(Dispatchers.IO) {
            val target = file(context, nasId, path, edge)
            if (!target.exists() || target.length() == 0L) return@withContext null
            try {
                target.readBytes()
            } catch (error: Exception) {
                Log.w(TAG, "表示用画像を読めなかった: $path", error)
                null
            }
        }

    /**
     * 原本のバイト列から作って置く。**大きな絵を丸ごとメモリに広げない。**
     *
     * 6000x4000 の写真をそのままデコードすると 96MB になる。まず大きさだけ
     * 読んで、必要な倍率まで間引いてからデコードする。
     */
    fun write(
        context: Context,
        nasId: String,
        path: String,
        edge: Int,
        original: ByteArray,
        orientation: Int
    ): Boolean = try {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(original, 0, original.size, bounds)
        val longest = maxOf(bounds.outWidth, bounds.outHeight)
        if (longest <= 0) {
            Log.w(TAG, "大きさを読めなかった: $path")
            false
        } else {
            var sample = 1
            // 目標の 2 倍を下回らないところまで間引く。間引きすぎると眠い絵になる。
            while (longest / (sample * 2) >= edge) sample *= 2
            val decoded = BitmapFactory.decodeByteArray(
                original, 0, original.size,
                BitmapFactory.Options().apply { inSampleSize = sample }
            )
            if (decoded == null) {
                Log.w(TAG, "デコードできなかった: $path")
                false
            } else {
                val scaled = scaleToEdge(decoded, edge)
                if (scaled !== decoded) decoded.recycle()
                val turned = SmbExifReader.applyOrientation(scaled, orientation)
                if (turned !== scaled) scaled.recycle()

                val target = File(dir(context), name(nasId, path, edge))
                val temporary = File(target.parentFile, "${target.name}.writing")
                temporary.outputStream().use {
                    turned.compress(Bitmap.CompressFormat.JPEG, 80, it)
                }
                turned.recycle()
                // **書き終えてから置き換える。** 途中で止まっても壊れた絵を残さない。
                if (!temporary.renameTo(target)) {
                    temporary.copyTo(target, overwrite = true)
                    temporary.delete()
                }
                true
            }
        }
    } catch (error: Exception) {
        // 1 枚で全体を止めない。**理由は必ず残す。**
        Log.w(TAG, "表示用画像を作れなかった: $path", error)
        false
    }

    private fun scaleToEdge(bitmap: Bitmap, edge: Int): Bitmap {
        val longest = maxOf(bitmap.width, bitmap.height)
        if (longest <= edge) return bitmap
        val ratio = edge.toFloat() / longest
        return Bitmap.createScaledBitmap(
            bitmap,
            (bitmap.width * ratio).toInt().coerceAtLeast(1),
            (bitmap.height * ratio).toInt().coerceAtLeast(1),
            true
        )
    }

    /** いま置いてある枚数。準備の進み具合に使う。 */
    fun count(context: Context, nasId: String, edge: Int): Int =
        dir(context).listFiles { file ->
            file.name.startsWith("${nasId}_") && file.name.endsWith("_$edge.jpg")
        }?.size ?: 0

    /** 置いてある量。**消すときに何 MB 消えるかを言うため。** */
    fun bytes(context: Context, nasId: String): Long =
        dir(context).listFiles { file -> file.name.startsWith("${nasId}_") }
            ?.sumOf { it.length() } ?: 0L

    suspend fun clear(context: Context, nasId: String) = withContext(Dispatchers.IO) {
        try {
            dir(context).listFiles { file -> file.name.startsWith("${nasId}_") }
                ?.forEach { it.delete() }
            Unit
        } catch (error: Exception) {
            Log.w(TAG, "表示用画像を片付けられなかった", error)
        }
    }
}
