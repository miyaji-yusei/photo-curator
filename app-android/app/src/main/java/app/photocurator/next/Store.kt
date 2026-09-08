package app.photocurator.next

import android.content.Context
import android.util.Log
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uniffi.photo_curator_core.PairOverride
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

    /**
     * 一覧に出すための、ごく短い言い方。**中身は読まずに済ませたい**ので、
     * ここだけは丸ごと読んで畳む。アルバムの数だけ小さな JSON を読む。
     */
    suspend fun summary(context: Context, albumId: String): String? {
        val session = load(context, albumId) ?: return null
        val stars = session.ratings.values.count { it > 0 }
        return when {
            !session.finished ->
                "ROUND ${session.round} の途中 · 残り ${session.queue.size + session.current.size} 組"
            stars > 0 -> "★1 以上が $stars 枚"
            else -> "選別ずみ"
        }
    }

    suspend fun clear(context: Context, albumId: String) = withContext(Dispatchers.IO) {
        file(context, albumId).delete()
        Unit
    }
}

/**
 * 1 枚ぶんの指紋と、それが**いつの原本のものか**。
 *
 * 大きさを控えるのは、写真が差し替わったときに古い指紋を使わないため。
 * 版を控えるのは、作り方を変えたときに黙って混ざらないため。
 */
data class Fingerprint(val version: Int, val size: Long, val hash: String)

/**
 * 指紋の置き場。**一度作ったものは作り直さない。**
 *
 * Tauri 版はプロジェクトを開くたびに解析し直していて、
 * 何が起きているのか誰にも分からなかった。作った結果は必ず残す。
 */
object Fingerprints {
    private const val TAG = "Fingerprints"

    private fun file(context: Context, albumId: String) =
        File(context.filesDir, "fingerprints-$albumId.json")

    suspend fun load(context: Context, albumId: String): Map<String, Fingerprint> =
        withContext(Dispatchers.IO) {
            val target = file(context, albumId)
            if (!target.exists()) return@withContext emptyMap()
            try {
                val root = org.json.JSONObject(target.readText())
                val out = HashMap<String, Fingerprint>(root.length())
                for (path in root.keys()) {
                    val entry = root.getJSONObject(path)
                    out[path] = Fingerprint(
                        version = entry.getInt("v"),
                        size = entry.getLong("size"),
                        hash = entry.getString("h")
                    )
                }
                out
            } catch (error: Exception) {
                // 読めないものは無かったことにして作り直す。**部分的に読まない。**
                Log.w(TAG, "指紋を読めなかった: $albumId", error)
                emptyMap()
            }
        }

    suspend fun save(context: Context, albumId: String, prints: Map<String, Fingerprint>) =
        withContext(Dispatchers.IO) {
            try {
                val root = org.json.JSONObject()
                for ((path, print) in prints) {
                    root.put(
                        path,
                        org.json.JSONObject()
                            .put("v", print.version)
                            .put("size", print.size)
                            .put("h", print.hash)
                    )
                }
                val target = file(context, albumId)
                val temporary = File(target.parentFile, "${target.name}.writing")
                temporary.writeText(root.toString())
                if (!temporary.renameTo(target)) {
                    temporary.copyTo(target, overwrite = true)
                    temporary.delete()
                }
                Unit
            } catch (error: Exception) {
                Log.w(TAG, "指紋を保存できなかった: $albumId", error)
            }
        }
}

/**
 * 覚えておく設定。**次に開いたときに同じ状態から始められるように。**
 *
 * 選別の途中（Session）とは別。あちらはアルバムごとの進み具合で、
 * こちらは「いつもこうしたい」というその人の好み。
 */
object Prefs {
    private const val FILE = "prefs"
    private const val GROUP_SIZE = "group_size"

    /** 一度に並べる枚数。2〜10。 */
    fun groupSize(context: Context): Int =
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
            .getInt(GROUP_SIZE, 4)
            .coerceIn(2, 10)

    fun setGroupSize(context: Context, size: Int) {
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
            .edit()
            .putInt(GROUP_SIZE, size.coerceIn(2, 10))
            .apply()
    }
}

/**
 * 人が手で直したまとめ方。**基準より優先され、基準を変えても残る。**
 *
 * 持つのは「この 2 枚のあいだを繋ぐか切るか」だけ。まとまりそのものは
 * そこから導く。まとまりを直接持つと、写真が増減したときに指し先を失う。
 */
object Overrides {
    private const val TAG = "Overrides"

    private fun file(context: Context, albumId: String) =
        File(context.filesDir, "overrides-$albumId.json")

    suspend fun load(context: Context, albumId: String): List<PairOverride> =
        withContext(Dispatchers.IO) {
            val target = file(context, albumId)
            if (!target.exists()) return@withContext emptyList()
            try {
                val array = org.json.JSONArray(target.readText())
                (0 until array.length()).map { at ->
                    val entry = array.getJSONObject(at)
                    PairOverride(
                        left = entry.getString("l"),
                        right = entry.getString("r"),
                        decision = entry.getString("d")
                    )
                }
            } catch (error: Exception) {
                // 読めないものは無かったことにする。**中途半端に読まない。**
                Log.w(TAG, "手直しを読めなかった: $albumId", error)
                emptyList()
            }
        }

    suspend fun save(context: Context, albumId: String, list: List<PairOverride>) =
        withContext(Dispatchers.IO) {
            try {
                val array = org.json.JSONArray()
                for (item in list) {
                    array.put(
                        org.json.JSONObject()
                            .put("l", item.left)
                            .put("r", item.right)
                            .put("d", item.decision)
                    )
                }
                val target = file(context, albumId)
                val temporary = File(target.parentFile, "${target.name}.writing")
                temporary.writeText(array.toString())
                if (!temporary.renameTo(target)) {
                    temporary.copyTo(target, overwrite = true)
                    temporary.delete()
                }
                Unit
            } catch (error: Exception) {
                Log.w(TAG, "手直しを保存できなかった: $albumId", error)
            }
        }

    /**
     * 新しい指定を足す。**同じ境目には答えを 1 つしか持たない。**
     * 2 つあると、どちらが効いているのか誰にも説明できなくなる。
     */
    fun merged(existing: List<PairOverride>, added: List<PairOverride>): List<PairOverride> {
        val replaced = added.associateBy { it.left to it.right }
        return existing.filterNot { (it.left to it.right) in replaced } + added
    }
}
