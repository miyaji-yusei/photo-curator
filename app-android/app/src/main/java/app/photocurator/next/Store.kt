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

    internal fun file(context: Context, projectId: String) =
        File(context.filesDir, "session-$projectId.json")

    /**
     * 保存する。**確定のたびに呼ばれる想定なので、失敗しても選別は止めない。**
     * 書けなかったことは記録する（黙って落とさない）。
     */
    suspend fun save(context: Context, projectId: String, session: Session) {
        // **保存はアプリの列で 1 本ずつ。最後に頼んだ状態が必ず残る**（A2）。
        // 確定を連打しても、同じ一時ファイルを奪い合わず、古い状態が新しい状態を戻さない。
        val target = file(context, projectId)
        Persist.latest("session:$projectId") {
            try {
                // 途中で落ちても壊れた JSON を残さないよう、書いてから差し替える。
                // rename が使えない環境ではコピーで置き換える。
                target.writeAtomically { it.writeText(sessionToJson(session)) }
            } catch (error: Exception) {
                Log.w(TAG, "選別の途中を保存できなかった: $projectId", error)
            }
        }
    }

    /** 読み戻す。**形が合わなければ null。** 最初からやり直してもらう。 */
    suspend fun load(context: Context, projectId: String): Session? =
        withContext(Dispatchers.IO) {
            val target = file(context, projectId)
            if (!target.exists()) return@withContext null
            try {
                sessionFromJson(target.readText())
            } catch (error: Exception) {
                Log.w(TAG, "選別の途中を読めなかった: $projectId", error)
                null
            }
        }

    /**
     * 一覧に出すための、ごく短い言い方。**中身は読まずに済ませたい**ので、
     * ここだけは丸ごと読んで畳む。アルバムの数だけ小さな JSON を読む。
     */
    suspend fun summary(context: Context, projectId: String): String? {
        val session = load(context, projectId) ?: return null
        val stars = session.ratings.values.count { it > 0 }
        return when {
            !session.finished ->
                "ROUND ${session.round} の途中 · 残り ${session.queue.size + session.current.size} 組"
            stars > 0 -> "★1 以上が $stars 枚"
            else -> "選別ずみ"
        }
    }

    suspend fun clear(context: Context, projectId: String) {
        // 保存と同じ列に載せる。**消したあとに、前の保存が書き戻さない。**
        val target = file(context, projectId)
        Persist.latest("session:$projectId") { target.delete() }
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
    internal fun file(context: Context, sourceKey: String) =
        File(context.filesDir, "fingerprints-${sourceKey.replace(Regex("[^A-Za-z0-9_-]"), "_")}.json")

    suspend fun load(context: Context, sourceKey: String): Map<String, Fingerprint> =
        withContext(Dispatchers.IO) {
            val target = file(context, sourceKey)
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
                Log.w(TAG, "指紋を読めなかった: $sourceKey", error)
                emptyMap()
            }
        }

    suspend fun save(context: Context, sourceKey: String, prints: Map<String, Fingerprint>) =
        Persist.latest("fingerprints:$sourceKey") {
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
                file(context, sourceKey).writeAtomically { it.writeText(root.toString()) }
            } catch (error: Exception) {
                Log.w(TAG, "指紋を保存できなかった: $sourceKey", error)
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

    /** スライドショー選別は「1 グループ 1 枚」で表す（トーナメントは 2 以上）。 */
    const val SLIDESHOW_SIZE = 1
    private const val TOURNAMENT_SIZE = "tournament_size"

    /**
     * 一度に並べる枚数。1〜10。**1 はスライドショー**（方式）。
     * 既定は 4。設計は Android 2〜4 だが、開いた状態は 933px あり
     * 実際に 10 枚を使うので広げる（設計自身も「幅と能力で決める」）。
     */
    fun groupSize(context: Context): Int =
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
            .getInt(GROUP_SIZE, 4)
            .coerceIn(SLIDESHOW_SIZE, 10)

    /** スライドショーから戻すときのトーナメントの枚数。**直前に選んでいた枚数。** */
    fun tournamentSize(context: Context): Int =
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
            .getInt(TOURNAMENT_SIZE, 4)
            .coerceIn(2, 10)

    /**
     * 表示用画像の長辺。**選別で見る絵の大きさ。**
     * 標準 1024（2,000 枚で約 170MB）／大きく 1536（約 318MB）。
     */
    fun displayEdge(context: Context): Int =
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
            .getInt("display_edge", 1024)
            .coerceIn(768, 1920)

    /**
     * 既定を変える。**効くのは新しいプロジェクトだけ**（設定画面にもそう書いてある）。
     * 自分の値をまだ持たない既存のプロジェクトには、変える前の既定をここで書き留める。
     * 書き留めないと、次に開いたとき新しい既定として扱われて、全部作り直しになる（A7）。
     */
    fun setDisplayEdge(context: Context, edge: Int) {
        val preferences = context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
        val old = displayEdge(context)
        val editor = preferences.edit()
        if (old != edge.coerceIn(768, 1920)) {
            // 設定の画面から呼ばれる。一覧のファイルは小さいので、その場で読む。
            val ids = kotlinx.coroutines.runBlocking { Projects.all(context) }.map { it.id }
            for (id in edgesToPin(ids) { preferences.contains("display_edge_$it") }) {
                editor.putInt("display_edge_$id", old)
            }
        }
        editor.putInt("display_edge", edge.coerceIn(768, 1920)).apply()
    }

    /** 自分の大きさをまだ持たないプロジェクト。 */
    fun edgesToPin(ids: List<String>, hasOwn: (String) -> Boolean): List<String> =
        ids.filterNot(hasOwn)

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

    /** 表示用画像の大きさの選択肢。**画面ごとに書き直さない。** */
    val EDGES = listOf(768, 1024, 1280, 1536, 1920)

    /**
     * Amazon が出せる長辺の上限（設計 08 章 8.5）。**0 はまだ測っていない。**
     * 共有リンクごとに 1 回だけ測る。
     */
    fun amazonMaxEdge(context: Context, shareId: String): Int =
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
            .getInt("amazon_max_edge_" + shareId, 0)

    fun setAmazonMaxEdge(context: Context, shareId: String, edge: Int) {
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
            .edit().putInt("amazon_max_edge_" + shareId, edge).apply()
    }

    /** その出所で出せる上限。**上限の無い出所は 0。** */
    fun maxEdgeFor(context: Context, source: Source): Int =
        if (source.kind == SourceKind.Amazon) amazonMaxEdge(context, Amazon.linkOf(source.key).shareId) else 0

    /** その大きさを選べるか。上限が分からなければ選べる。 */
    fun edgeAllowed(edge: Int, max: Int): Boolean = max <= 0 || edge <= max

    /** 上限を超えない一番大きい選択肢。**超えていなければそのまま。** */
    fun usableEdge(edge: Int, max: Int): Int =
        if (edgeAllowed(edge, max)) edge else EDGES.filter { it <= max }.maxOrNull() ?: EDGES.first()

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

    /**
     * 選別中に写真を長押ししたときの動き。**既定は「選ぶ」**（off）。
     *
     * 拡大はタイルの虫眼鏡にもあるが、複数選びは長押しでしか始められない。
     * 拡大の方が好きな人のために、設定で戻せるようにしてある。
     */
    fun holdZooms(context: Context): Boolean =
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
            .getBoolean("hold_zooms", false)

    fun setHoldZooms(context: Context, on: Boolean) {
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
            .edit().putBoolean("hold_zooms", on).apply()
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
        val kept = size.coerceIn(SLIDESHOW_SIZE, 10)
        val edit = context.getSharedPreferences(FILE, Context.MODE_PRIVATE).edit()
            .putInt(GROUP_SIZE, kept)
        // 1（スライドショー）に切り替えても、トーナメントの枚数は覚えておく。
        if (kept >= 2) edit.putInt(TOURNAMENT_SIZE, kept)
        edit.apply()
    }

    /** プロジェクトを消すときに、そのプロジェクトだけの設定を片付ける。 */
    fun forgetProject(context: Context, projectId: String) {
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
            .edit().remove("display_edge_" + projectId).apply()
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

    internal fun file(context: Context, projectId: String) =
        File(context.filesDir, "overrides-$projectId.json")

    suspend fun load(context: Context, projectId: String): List<PairOverride> =
        withContext(Dispatchers.IO) {
            val target = file(context, projectId)
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
                Log.w(TAG, "手直しを読めなかった: $projectId", error)
                emptyList()
            }
        }

    /** 手直しを全部消す。**やり直しのときだけ。** */
    suspend fun clear(context: Context, projectId: String) {
        Persist.latest("overrides:$projectId") { file(context, projectId).delete() }
    }

    suspend fun save(context: Context, projectId: String, list: List<PairOverride>) =
        Persist.latest("overrides:$projectId") {
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
                file(context, projectId).writeAtomically { it.writeText(array.toString()) }
            } catch (error: Exception) {
                Log.w(TAG, "手直しを保存できなかった: $projectId", error)
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

    internal fun file(context: Context, sourceKey: String) =
        File(context.filesDir, "listing-${sourceKey.replace(Regex("[^A-Za-z0-9_-]"), "_")}.json")

    suspend fun load(context: Context, sourceKey: String): List<Photo>? = withContext(Dispatchers.IO) {
        val target = file(context, sourceKey)
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
                    remote = when {
                        entry.has("nas") -> SmbRef(entry.getString("nas"), entry.getString("path"))
                        entry.has("amz") -> AmazonRef(entry.getString("amz"), entry.getString("node"), entry.optString("tl"))
                        else -> null
                    }
                )
            }
        } catch (error: Exception) {
            Log.w(TAG, "顔ぶれを読めなかった: $sourceKey", error)
            null
        }
    }

    suspend fun save(context: Context, sourceKey: String, photos: List<Photo>) =
        Persist.latest("listing:$sourceKey") {
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
                                photo.amazon?.let {
                                    put("amz", it.shareKey); put("node", it.nodeId); put("tl", it.tempLink)
                                }
                            }
                    )
                }
                file(context, sourceKey).writeAtomically { it.writeText(array.toString()) }
            } catch (error: Exception) {
                Log.w(TAG, "顔ぶれを保存できなかった: $sourceKey", error)
            }
        }

    suspend fun clear(context: Context, sourceKey: String) {
        Persist.latest("listing:$sourceKey") { file(context, sourceKey).delete() }
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

    internal fun file(context: Context, sourceKey: String) =
        File(context.filesDir, "trouble-${sourceKey.replace(Regex("[^A-Za-z0-9_-]"), "_")}.txt")

    suspend fun note(context: Context, sourceKey: String, message: String) =
        withContext(Dispatchers.IO) {
            try {
                file(context, sourceKey).writeText(message)
            } catch (error: Exception) {
                Log.w(TAG, "困りごとを書けなかった: $sourceKey", error)
            }
        }

    suspend fun load(context: Context, sourceKey: String): String? = withContext(Dispatchers.IO) {
        val target = file(context, sourceKey)
        if (!target.exists()) return@withContext null
        try {
            target.readText().takeIf { it.isNotBlank() }
        } catch (error: Exception) {
            null
        }
    }

    suspend fun clear(context: Context, sourceKey: String) = withContext(Dispatchers.IO) {
        try {
            file(context, sourceKey).delete()
        } catch (error: Exception) {
            Log.w(TAG, "困りごとを消せなかった: $sourceKey", error)
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
 * サイドカーと端末の食い違いを見分けるための控え（U35 で core の `sidecarPlan` 用に置き換えた）。
 *
 * **時刻の大小で勝敗を決めない。** 見るのは「自分が最後に見た版（token）と同じか」と、
 * 「その版の選別状況の比較キー（key）と、今の端末の比較キーが同じか」。
 * 「変更があるか」は印（dirty）ではなく中身で決める（core）。印を立てる・消す操作が無いので、
 * 書いている最中の判断の印を消す・キャンセルで印が落ちる、が起きない。
 *
 * 置き場所は SharedPreferences「sync」。キーは `<プロジェクト>-seenToken` など。
 * 古い控え（`-seenAt`・`-seenBy`・`-dirty`）は、最初に読んだときに移す（設計書 §5 の PR-5）。
 */
object SyncState {
    private const val FILE = "sync"

    private fun prefs(context: Context) =
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE)

    /** [SidecarSync] に渡す控えの置き場所。 */
    fun store(context: Context): SeenStore {
        val app = context.applicationContext
        return object : SeenStore {
            override fun load(projectId: String): SeenState = load(app, projectId)
            override fun save(projectId: String, state: SeenState) = save(app, projectId, state)
        }
    }

    fun load(context: Context, projectId: String): SeenState {
        val store = prefs(context)
        val token = store.getString("$projectId-seenToken", null)
        if (token != null) {
            return SeenState(
                uniffi.photo_curator_core.SeenRecord(
                    token,
                    store.getString("$projectId-seenKey", "") ?: "",
                    store.getString("$projectId-seenEpoch", null)
                ),
                detached = store.getBoolean("$projectId-detached", false)
            )
        }
        // 古い控えからの移し替え。seenAt/seenBy は core の legacy の token と同じ形にする。
        // dirty=true なら比較キーを空にして「変更あり」、false なら今の端末の比較キーで埋める。
        val seenAt = store.getLong("$projectId-seenAt", -1L)
        if (seenAt < 0) return SeenState(uniffi.photo_curator_core.SeenRecord("", "", null), detached = false)
        val seenBy = store.getString("$projectId-seenBy", "") ?: ""
        val dirty = store.getBoolean("$projectId-dirty", false)
        return SeenState(
            uniffi.photo_curator_core.SeenRecord("legacy:$seenAt:$seenBy", "", null),
            detached = false,
            keyFromLocal = !dirty
        )
    }

    fun save(context: Context, projectId: String, state: SeenState) {
        prefs(context).edit()
            .putString("$projectId-seenToken", state.seen.token)
            .putString("$projectId-seenKey", state.seen.key)
            .putString("$projectId-seenEpoch", state.seen.epoch)
            .putBoolean("$projectId-detached", state.detached)
            // 古い控えは移したので消す（残すと、消したあとの移し替えで古い版に戻る）。
            .remove("$projectId-seenAt")
            .remove("$projectId-seenBy")
            .remove("$projectId-dirty")
            .commit()
    }

    /** この端末のやり直しの世代。**やり直したことを、ほかの端末に伝えるため**（設計書 §6 の Q10）。 */
    fun epoch(context: Context, projectId: String): String? =
        prefs(context).getString("$projectId-epoch", null)

    fun setEpoch(context: Context, projectId: String, epoch: String?) {
        prefs(context).edit().putString("$projectId-epoch", epoch).commit()
    }

    /** やり直した。**新しい世代にする**（相手の進んだ分で黙って埋め戻されないように）。 */
    fun restarted(context: Context, projectId: String) {
        setEpoch(context, projectId, "e-" + java.util.UUID.randomUUID().toString().replace("-", "").take(16))
    }

    fun forget(context: Context, projectId: String) {
        val store = prefs(context)
        val gone = store.all.keys.filter { it.startsWith("$projectId-") }
        if (gone.isEmpty()) return
        store.edit().apply { for (k in gone) remove(k) }.apply()
    }
}
