package app.photocurator.next

import android.content.Context
import android.util.Log
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.withContext

/**
 * 見本の絵をそろえる。**「どのフォルダだったか」を思い出すための 1 枚。**
 *
 * 名前と枚数だけでは、フォルダを選ぶときに中身を思い出せない。かといって
 * 選ぶ前の画面で原本を読むのは高すぎる（NAS の原本は 1 枚 6MB）。
 * **EXIF の中の 160×120 だけ**を、**1 本の接続でまとめて**取る。
 *
 * 1 枚ごとに接続を張り直すと 1 枚 800ms かかる（実測 40 秒 / 50 枚）。
 * まとめれば 4 枚/秒まで上がる（準備の実測）。ここは選ぶ前の画面なので、
 * **待たせるくらいなら出さない**方に倒す。
 */
object Covers {
    private const val TAG = "Covers"

    /** 下の帯に出す枚数の上限。**中身を思い出せれば十分。** */
    const val STRIP = 12

    /**
     * NAS の写真のサムネイルを、端末に無いものだけまとめて取る。
     *
     * 取れたものは `thumbs/` に置くので、**2 回目からは網へ行かない**。
     * 取れなかったものは黙って飛ばす（選ぶ前の画面で失敗を並べても、
     * できることが無い）。
     */
    suspend fun warm(context: Context, nas: Nas, password: String, paths: List<String>) =
        withContext(Dispatchers.IO) {
            val wanted = paths.filterNot { ThumbCache.has(context, nas.id, it) }
            if (wanted.isEmpty()) return@withContext
            val began = System.currentTimeMillis()
            var made = 0
            Smb.reading(nas, password) { reader ->
                // 4 枚ずつ重ねる。**1 本の接続の中で並べる**ので、
                // 接続の張り直しは起きない（準備と同じやり方）。
                for (chunk in wanted.chunked(4)) {
                    coroutineScope {
                        chunk.map { path ->
                            async {
                                val head = reader.head(path, SmbExifReader.HEAD_BYTES)
                                    ?: return@async false
                                val exif = SmbExifReader.parse(head, 0L)
                                val bytes = exif.thumbnail ?: return@async false
                                val decoded = android.graphics.BitmapFactory
                                    .decodeByteArray(bytes, 0, bytes.size) ?: return@async false
                                ThumbCache.write(
                                    context, nas.id, path,
                                    SmbExifReader.applyOrientation(decoded, exif.orientation)
                                )
                                true
                            }
                        }.forEach { if (it.await()) made += 1 }
                    }
                }
            }
            Log.i(TAG, "見本 $made / ${wanted.size} 枚を ${System.currentTimeMillis() - began}ms")
        }

    /**
     * ホームのカードに出す 1 枚。**端末にあるものだけ。網へは行かない。**
     *
     * ホームを開くたびに NAS へ行くと、一覧が出るまで待たされる。準備で
     * 作ったサムネイルが残っていればそれを出し、無ければ何も出さない。
     */
    suspend fun forProject(context: Context, project: Project): Any? =
        withContext(Dispatchers.IO) {
            val known = Listing.load(context, project.source.key) ?: return@withContext null
            val first = known.firstOrNull() ?: return@withContext null
            // Amazon も端末に残っているサムネイルだけ。**取りに行かない。**
            first.amazon?.let { ref ->
                val file = ThumbCache.file(context, Amazon.linkOf(ref.shareKey).cacheId, ref.nodeId)
                return@withContext if (file.exists() && file.length() > 0) file else null
            }
            val smb = first.smb
            if (smb == null) return@withContext first.uri
            // NAS は端末に残っているときだけ。**取りに行かない。**
            val file = ThumbCache.file(context, smb.nasId, smb.path)
            if (file.exists() && file.length() > 0) file else null
        }
}
