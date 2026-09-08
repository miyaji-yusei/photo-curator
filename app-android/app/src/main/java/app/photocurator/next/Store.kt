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
            // **判断が変わった。** サイドカーへ渡すべきものが端末にできた印。
            SyncState.touch(context, albumId)
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
data class Fingerprint(
    val version: Int,
    val size: Long,
    val hash: String,
    /**
     * 撮影時刻。**NAS のときだけ入る。**
     * 端末は MediaStore が持っているが、NAS は EXIF を読まないと分からない。
     * 指紋と同じ 1 回の読みで取れるので、一緒に控える。
     */
    val takenAt: Long? = null
)

/**
 * 指紋の置き場。**一度作ったものは作り直さない。**
 *
 * Tauri 版はプロジェクトを開くたびに解析し直していて、
 * 何が起きているのか誰にも分からなかった。作った結果は必ず残す。
 */
object Fingerprints {
    private const val TAG = "Fingerprints"

    /**
     * 出所の鍵をそのままファイル名にしない。
     * NAS の鍵は "nasId|フォルダ道筋" の形で、区切り記号がそのまま入ると
     * **扱いにくい名前のファイル**ができる。英数字以外は _ に潰す。
     */
    private fun file(context: Context, key: String) =
        File(context.filesDir, "fingerprints-${key.replace(Regex("[^A-Za-z0-9_-]"), "_")}.json")

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
                        hash = entry.getString("h"),
                        takenAt = if (entry.has("t")) entry.getLong("t") else null
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
                            .apply { print.takenAt?.let { put("t", it) } }
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
            // 2〜10。既定は 4。設計は Android 2〜4 だが、開いた状態は 933px あり
            // 実際に 10 枚を使うので広げる（設計自身も「幅と能力で決める」）。
            .coerceIn(2, 10)

    /**
     * 表示用画像の長辺。**選別で見る絵の大きさ。**
     * 標準 1024（2,000 枚で約 170MB）／大きく 1536（約 318MB）。
     */
    fun displayEdge(context: Context): Int =
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
            .getInt("display_edge", 1024)
            .coerceIn(768, 1920)

    fun setDisplayEdge(context: Context, edge: Int) {
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
            .edit().putInt("display_edge", edge.coerceIn(768, 1920)).apply()
    }

    /**
     * プロジェクトごとの長辺。**設定の値は「新しいプロジェクトの既定」**で、
     * 途中で大きさを変えたいのは目の前の 1 つだけ、ということが多い。
     * 決めていなければ既定に従う。
     */
    fun projectEdge(context: Context, projectId: String): Int =
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
            .getInt("display_edge_" + projectId, displayEdge(context))
            .coerceIn(768, 1920)

    fun setProjectEdge(context: Context, projectId: String, edge: Int) {
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
            .edit().putInt("display_edge_" + projectId, edge.coerceIn(768, 1920)).apply()
    }

    /**
     * プロジェクト詳細の一覧の列数。**0 は「おまかせ」**（幅から決める）。
     * 覚えておくのは、開くたびに選び直したくないため。
     */
    fun gridColumns(context: Context): Int =
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
            .getInt("grid_columns", 0).coerceIn(0, 8)

    fun setGridColumns(context: Context, columns: Int) {
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
            .edit().putInt("grid_columns", columns.coerceIn(0, 8)).apply()
    }

    /** 連写を自動でまとめるか。既定 on。 */
    fun groupBursts(context: Context): Boolean =
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
            .getBoolean("group_bursts", true)

    fun setGroupBursts(context: Context, on: Boolean) {
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
            .edit().putBoolean("group_bursts", on).apply()
    }

    /** 開始前に毎回設定を確かめるか。**既定 off。** 選別中にも変えられるため。 */
    fun askBeforeStart(context: Context): Boolean =
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
            .getBoolean("ask_before_start", false)

    fun setAskBeforeStart(context: Context, on: Boolean) {
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
            .edit().putBoolean("ask_before_start", on).apply()
    }

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

    /** 手直しを全部消す。**やり直しのときだけ。** */
    suspend fun clear(context: Context, albumId: String) = withContext(Dispatchers.IO) {
        file(context, albumId).delete()
        Unit
    }

    suspend fun save(context: Context, albumId: String, list: List<PairOverride>) =
        withContext(Dispatchers.IO) {
            // 手直しも判断。**サイドカーへ渡すもの。**
            SyncState.touch(context, albumId)
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

/**
 * 写真の顔ぶれを控えておく。
 *
 * **開くたびに数え直さない。** NAS では一覧を取るだけで網の往復が要る。
 * 中身が変わっていなければ前の結果でよい。
 *
 * 変わったかどうかは、こちらからは分からない。だから**自動では見に行かず**、
 * 「写真を再読み込み」を押されたときだけ取り直す。勝手に取り直すと、
 * 開くたびに待たされる理由が誰にも分からなくなる。
 */
object Listing {
    private const val TAG = "Listing"

    private fun file(context: Context, key: String) =
        File(context.filesDir, "listing-${key.replace(Regex("[^A-Za-z0-9_-]"), "_")}.json")

    suspend fun load(context: Context, key: String): List<Photo>? = withContext(Dispatchers.IO) {
        val target = file(context, key)
        if (!target.exists()) return@withContext null
        try {
            val array = org.json.JSONArray(target.readText())
            (0 until array.length()).map { at ->
                val entry = array.getJSONObject(at)
                Photo(
                    id = entry.getLong("id"),
                    name = entry.getString("name"),
                    relativePath = entry.getString("rel"),
                    size = entry.getLong("size"),
                    takenAt = entry.getLong("at"),
                    smb = if (entry.has("nas")) {
                        SmbRef(entry.getString("nas"), entry.getString("path"))
                    } else null
                )
            }
        } catch (error: Exception) {
            Log.w(TAG, "顔ぶれを読めなかった: $key", error)
            null
        }
    }

    suspend fun save(context: Context, key: String, photos: List<Photo>) =
        withContext(Dispatchers.IO) {
            try {
                val array = org.json.JSONArray()
                for (photo in photos) {
                    array.put(
                        org.json.JSONObject()
                            .put("id", photo.id)
                            .put("name", photo.name)
                            .put("rel", photo.relativePath)
                            .put("size", photo.size)
                            .put("at", photo.takenAt)
                            .apply {
                                photo.smb?.let { put("nas", it.nasId); put("path", it.path) }
                            }
                    )
                }
                val target = file(context, key)
                val temporary = File(target.parentFile, "${target.name}.writing")
                temporary.writeText(array.toString())
                if (!temporary.renameTo(target)) {
                    temporary.copyTo(target, overwrite = true)
                    temporary.delete()
                }
                Unit
            } catch (error: Exception) {
                Log.w(TAG, "顔ぶれを保存できなかった: $key", error)
            }
        }

    suspend fun clear(context: Context, key: String) = withContext(Dispatchers.IO) {
        file(context, key).delete()
        Unit
    }
}

/**
 * 準備でつまずいたことを控える。**ホームで理由を出すため。**
 *
 * ホームは開いても網へ行かない（行くと一覧が出るまで待たされる）ので、
 * 「NAS に届きません」を自分で確かめる術がない。だから**転んだ側が
 * 書き残す**。次に準備が通ったら消す。
 */
object Trouble {
    private const val TAG = "Trouble"

    private fun file(context: Context, key: String) =
        File(context.filesDir, "trouble-${key.replace(Regex("[^A-Za-z0-9_-]"), "_")}.txt")

    suspend fun note(context: Context, key: String, message: String) =
        withContext(Dispatchers.IO) {
            try {
                file(context, key).writeText(message)
            } catch (error: Exception) {
                Log.w(TAG, "困りごとを書けなかった: $key", error)
            }
        }

    suspend fun load(context: Context, key: String): String? = withContext(Dispatchers.IO) {
        val target = file(context, key)
        if (!target.exists()) return@withContext null
        try {
            target.readText().takeIf { it.isNotBlank() }
        } catch (error: Exception) {
            null
        }
    }

    suspend fun clear(context: Context, key: String) = withContext(Dispatchers.IO) {
        try {
            file(context, key).delete()
        } catch (error: Exception) {
            Log.w(TAG, "困りごとを消せなかった: $key", error)
        }
        Unit
    }
}

/**
 * ラウンドに掛かった時間を測る。**手が止まっていた時間は数えない。**
 *
 * 開始から完了までの時計をそのまま出すと、途中で寝て翌朝続けたときに
 * 「所要 9 時間」になる。それは所要時間ではない。**確定と確定のあいだ**を
 * 足し、間が開きすぎたところ（5 分以上）はそこで手が止まっていたとみなして
 * 数えない。
 */
object Timing {
    private const val TAG = "Timing"

    /** これ以上空いたら「見ていなかった」とみなす。 */
    private const val IDLE_MS = 5 * 60 * 1000L

    private fun prefs(context: Context) =
        context.getSharedPreferences("timing", Context.MODE_PRIVATE)

    private fun key(projectId: String, round: UInt) = "$projectId-$round"

    /** 1 回確定するたびに呼ぶ。前回からの間を足す。 */
    fun tick(context: Context, projectId: String, round: UInt) {
        val store = prefs(context)
        val at = key(projectId, round)
        val now = System.currentTimeMillis()
        val last = store.getLong("$at-last", 0L)
        val sum = store.getLong("$at-sum", 0L)
        val gap = if (last > 0) now - last else 0L
        val added = if (gap in 1..IDLE_MS) gap else 0L
        store.edit().putLong("$at-last", now).putLong("$at-sum", sum + added).apply()
    }

    /** そのラウンドに掛かった時間（ミリ秒）。まだ何も測っていなければ 0。 */
    fun spent(context: Context, projectId: String, round: UInt): Long =
        prefs(context).getLong("${key(projectId, round)}-sum", 0L)

    /** やり直したときは全部捨てる。 */
    fun clear(context: Context, projectId: String) {
        val store = prefs(context)
        val gone = store.all.keys.filter { it.startsWith("$projectId-") }
        if (gone.isEmpty()) return
        store.edit().apply { for (k in gone) remove(k) }.apply()
    }

    /** 「9 分 40 秒」。**1 分未満は秒だけ、1 時間を超えたら時間と分。** */
    fun describe(ms: Long): String {
        val seconds = ms / 1000
        return when {
            seconds < 60 -> "$seconds 秒"
            seconds < 3600 -> "${seconds / 60} 分 ${seconds % 60} 秒"
            else -> "${seconds / 3600} 時間 ${(seconds % 3600) / 60} 分"
        }
    }
}

/**
 * この端末の名札。**サイドカーで「誰が書いたか」を言うために要る。**
 *
 * 端末ごとに 1 回だけ作る。人が見て分かる名前（機種名）と、機械が比べる id。
 * id を機種名にしないのは、同じ機種が 2 台あると見分けが付かないため。
 */
object Device {
    private const val FILE = "device"

    fun id(context: Context): String {
        val store = context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
        store.getString("id", null)?.let { return it }
        val made = java.util.UUID.randomUUID().toString().take(12)
        store.edit().putString("id", made).apply()
        return made
    }

    /** 「SM-F971C」。人が見比べるときの手がかり。 */
    fun name(): String = android.os.Build.MODEL ?: "この端末"
}

/**
 * サイドカーと端末の食い違いを見分けるための控え。
 *
 * **時刻の大小で勝敗を決めない。** 端末ごとに時計はずれるので、
 * 「新しい方を採る」は狂った端末が常に勝つ。見るのは
 * 「**自分が最後に見た版と同じかどうか**」だけ。
 *
 * `changed` が動くのは**判断が変わったときだけ**（星・まとまりの手直し・
 * 学習した基準・やり直し）。準備（指紋や表示用画像）では動かさない。
 * 動かすと、見ただけで食い違い扱いになる。
 */
object SyncState {
    private const val FILE = "sync"

    private fun prefs(context: Context) =
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE)

    fun seenAt(context: Context, projectId: String): Long =
        prefs(context).getLong("$projectId-seenAt", -1L)

    fun seenBy(context: Context, projectId: String): String =
        prefs(context).getString("$projectId-seenBy", "") ?: ""

    /** 端末側に、まだ共有していない判断があるか。 */
    fun changed(context: Context, projectId: String): Boolean =
        prefs(context).getBoolean("$projectId-dirty", false)

    /** 判断が変わった。**ここでしか印を付けない。** */
    fun touch(context: Context, projectId: String) {
        prefs(context).edit().putBoolean("$projectId-dirty", true).apply()
    }

    /** 読んだ／書いた版を控える。**書けたときだけ呼ぶ。** */
    fun saw(context: Context, projectId: String, updatedAt: Long, updatedBy: String) {
        prefs(context).edit()
            .putLong("$projectId-seenAt", updatedAt)
            .putString("$projectId-seenBy", updatedBy)
            .putBoolean("$projectId-dirty", false)
            .apply()
    }

    fun clean(context: Context, projectId: String) {
        prefs(context).edit().putBoolean("$projectId-dirty", false).apply()
    }

    fun forget(context: Context, projectId: String) {
        val store = prefs(context)
        val gone = store.all.keys.filter { it.startsWith("$projectId-") }
        if (gone.isEmpty()) return
        store.edit().apply { for (k in gone) remove(k) }.apply()
    }
}
