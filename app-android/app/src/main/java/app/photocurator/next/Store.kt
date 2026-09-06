package app.photocurator.next

import android.content.Context
import android.util.Log
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uniffi.photo_curator_core.Session
import uniffi.photo_curator_core.sessionFromJson
import uniffi.photo_curator_core.sessionToJson
import java.io.File

/**
 * 選別の途中を保存する。
 *
 * **形は core が持つ。** ここは置き場所だけを決める。同じ形をそのまま
 * NAS のサイドカー（catalog.json）にも入れるので、端末をまたげる。
 *
 * いまはアルバムごとに 1 ファイル。**どこで止めても失わない**よう、
 * グループを確定するたびに書く。
 */
object Store {
    private const val TAG = "Store"

    private fun file(context: Context, albumId: String) =
        File(context.filesDir, "session-$albumId.json")

    /**
     * 保存する。**確定のたびに呼ばれる想定なので、失敗しても選別は止めない。**
     * 書けなかったことは記録する（黙って落とさない）。
     */
    suspend fun save(context: Context, albumId: String, session: Session) =
        withContext(Dispatchers.IO) {
            try {
                // 途中で落ちても壊れた JSON を残さないよう、書いてから差し替える。
                val target = file(context, albumId)
                val temporary = File(target.parentFile, "${target.name}.writing")
                temporary.writeText(sessionToJson(session))
                // rename が使えない環境ではコピーで置き換える。
                if (!temporary.renameTo(target)) {
                    temporary.copyTo(target, overwrite = true)
                    temporary.delete()
                }
                Unit
            } catch (error: Exception) {
                Log.w(TAG, "選別の途中を保存できなかった: $albumId", error)
            }
        }

    /** 読み戻す。**形が合わなければ null。** 最初からやり直してもらう。 */
    suspend fun load(context: Context, albumId: String): Session? =
        withContext(Dispatchers.IO) {
            val target = file(context, albumId)
            if (!target.exists()) return@withContext null
            try {
                sessionFromJson(target.readText())
            } catch (error: Exception) {
                Log.w(TAG, "選別の途中を読めなかった: $albumId", error)
                null
            }
        }

    suspend fun clear(context: Context, albumId: String) = withContext(Dispatchers.IO) {
        file(context, albumId).delete()
        Unit
    }
}
