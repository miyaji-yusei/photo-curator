package app.photocurator.next

import android.content.Context
import android.util.Log
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.File

/**
 * プロジェクト。**選別の単位。**
 *
 * 端末のアルバムを直接開くのではなく、「どのフォルダを選別するか」を
 * 人が決めて名前を付けたものを単位にする。同じアルバムから 2 つ作れるし、
 * NAS のフォルダも同じ形で扱える。出所が変わっても選別の続きは残る。
 */

/**
 * 出所の種類。**文字列ではなく型で持つ**（設計 09 章 §5 #2）。
 * 種類を足したときに `when` の漏れをコンパイルで検出できるようにする。
 * 保存する JSON には [id] をそのまま書く。**過去に書いた値は変えない。**
 */
enum class SourceKind(val id: String) {
    Album("album"),
    Nas("nas"),
    Amazon("amazon");

    companion object {
        fun fromId(id: String): SourceKind = entries.first { it.id == id }
    }
}

/**
 * 出所。**人の言葉（label）と、機械の指し先を分けて持つ。**
 * 生のパスや ID は「技術情報」にだけ出す。
 */
data class Source(
    val kind: SourceKind,
    /** 人に見せる言い方。「この端末・アルバム「Camera」」など。 */
    val label: String,
    /** kind ごとの指し先。album なら bucket id。 */
    val key: String
) {
    /**
     * 「このフォルダ以下ぜんぶ」か。
     *
     * 鍵の末尾に印を足すだけにしてある。出所の形（kind）を増やすと、
     * 保存してあるプロジェクトの読み方まで変わるので、そこは触らない。
     */
    val deep: Boolean get() = key.endsWith("|**")

    /** 印を外した、実際のフォルダの道筋。 */
    val folder: String get() = key.removeSuffix("|**").substringAfter("|")

    /** 技術情報に出す生の値。**普段は見せない。** */
    val technical: String get() = "${kind.id}:$key"

    /**
     * 網越しか。**NAS も Amazon も「表示用画像を作って置く」側。**
     * 「NAS か端末か」の 2 択で書かれていた分岐のうち、この意味のものはこれを見る。
     */
    val remote: Boolean get() = kind != SourceKind.Album

    /**
     * 端末に置いた絵（サムネイル・表示用画像）の名前の頭。
     * 端末のアルバムは絵を置かないので null。
     */
    val cacheId: String?
        get() = when (kind) {
            SourceKind.Nas -> key.substringBefore("|")
            SourceKind.Amazon -> Amazon.linkOf(key).cacheId
            SourceKind.Album -> null
        }
}

data class Project(
    val id: String,
    val name: String,
    val source: Source,
    val createdAt: Long,
    val updatedAt: Long
)

object Projects {
    private const val TAG = "Projects"
    private const val FILE = "projects.json"

    private fun file(context: Context) = File(context.filesDir, FILE)

    /** 読んだ結果。[unknown] は読めなかった 1 件ずつの JSON（書き戻すときに落とさない）。 */
    internal data class Parsed(val projects: List<Project>, val unknown: List<String>)

    /**
     * 読む。**1 件ずつ読み、読めない 1 件で全体を空にしない**（U50・A17 (a)）。
     * 全体が JSON の配列として読めなければ例外（呼ぶ側が写しを残す）。
     */
    internal fun parse(text: String): Parsed {
        val array = org.json.JSONArray(text)
        val projects = ArrayList<Project>()
        val unknown = ArrayList<String>()
        for (at in 0 until array.length()) {
            try {
                val entry = array.getJSONObject(at)
                val source = entry.getJSONObject("source")
                projects += Project(
                    id = entry.getString("id"),
                    name = entry.getString("name"),
                    source = Source(
                        kind = SourceKind.fromId(source.getString("kind")),
                        label = source.getString("label"),
                        key = source.getString("key")
                    ),
                    createdAt = entry.getLong("created"),
                    updatedAt = entry.getLong("updated")
                )
            } catch (error: Exception) {
                // 新しい版が書いた種類・欠けた 1 件。**捨てずに中身のまま持っておく。**
                unknown += array.get(at).toString()
            }
        }
        // **更新順。** 2 回目以降は続きから始めることの方が多い。
        return Parsed(projects.sortedByDescending { it.updatedAt }, unknown)
    }

    /** 書く形。[unknown]（読めなかった 1 件）は、そのまま後ろに付ける。 */
    internal fun serialize(projects: List<Project>, unknown: List<String>): String {
        val array = org.json.JSONArray()
        for (project in projects) {
            array.put(
                org.json.JSONObject()
                    .put("id", project.id)
                    .put("name", project.name)
                    .put("created", project.createdAt)
                    .put("updated", project.updatedAt)
                    .put(
                        "source",
                        org.json.JSONObject()
                            .put("kind", project.source.kind.id)
                            .put("label", project.source.label)
                            .put("key", project.source.key)
                    )
            )
        }
        for (raw in unknown) {
            try {
                array.put(org.json.JSONTokener(raw).nextValue())
            } catch (error: Exception) {
                array.put(raw)
            }
        }
        return array.toString()
    }

    /** 最後に読んだときの、読めなかった 1 件ずつ。[save] で書き戻す。 */
    @Volatile
    private var unknownSeen: List<String> = emptyList()

    suspend fun all(context: Context): List<Project> = withContext(Dispatchers.IO) {
        // **全体が読めなければ、元のファイルの写しを `projects.broken-<時刻>.json` に残す**（D2 と同じ）。
        // 空の一覧に新しい 1 件を足して保存しても、前の中身は写しに残る。
        when (val read = file(context).readStored(Parsed(emptyList(), emptyList())) { parse(it) }) {
            is Stored.Ok -> {
                if (read.value.unknown.isNotEmpty()) {
                    Log.w(TAG, "読めないプロジェクトが ${read.value.unknown.size} 件（書き戻すときも残す）")
                }
                unknownSeen = read.value.unknown
                unsafeToWrite = false
                lastSeen = read.value.projects
                read.value.projects
            }
            is Stored.Broken -> {
                Log.w(TAG, "プロジェクトを読めなかった: ${read.reason}。控え: ${read.keptAs?.name}")
                unknownSeen = emptyList()
                // 写しを残せなかった（読み込みそのものの失敗など）なら、**上書きしない**。
                // 空の一覧に 1 件足して書くと、前の中身がどこにも残らない。
                unsafeToWrite = read.keptAs == null && file(context).exists()
                emptyList()
            }
        }
    }

    /** 前の中身をどこにも残せていないので、書くと失う。次に読めるまで書かない。 */
    @Volatile
    private var unsafeToWrite = false

    suspend fun save(context: Context, projects: List<Project>) = withContext(Dispatchers.IO) {
        if (unsafeToWrite) {
            Log.w(TAG, "プロジェクトの一覧を読めていないので、上書きしない")
            return@withContext
        }
        try {
            val text = serialize(projects, unknownSeen)
            file(context).writeAtomically { it.writeText(text) }
        } catch (error: Exception) {
            Log.w(TAG, "プロジェクトを保存できなかった", error)
        }
    }

    /**
     * 最後に読んだ一覧。**背面へ回るときに、どれを書けばよいかを知るため。**
     * そのときにファイルを読みに行くと、止められる前に間に合わないことがある。
     */
    @Volatile
    private var lastSeen: List<Project> = emptyList()

    fun cached(): List<Project> = lastSeen

    suspend fun add(context: Context, name: String, source: Source, pairRaw: Boolean = true): Project {
        val now = System.currentTimeMillis()
        val project = Project("p$now", name, source, now, now)
        // **作った時点の既定をこのプロジェクトの大きさにする。** 決めないままだと、あとで
        // 設定の既定を変えたとき、このプロジェクトの表示用画像まで作り直しになる（A7）。
        Prefs.setProjectEdge(context, project.id, Prefs.displayEdge(context))
        // U49: 作成の画面でオフにしたときだけ書く（オンは既定。時刻 0 =「既定のまま」）。
        if (!pairRaw) Prefs.setPairRawJpeg(context, project.id, false, now)
        save(context, listOf(project) + all(context))
        return project
    }

    /** 触ったことを記録する。一覧の並び（更新順）に効く。 */
    suspend fun touch(context: Context, id: String) {
        val now = System.currentTimeMillis()
        save(context, all(context).map { if (it.id == id) it.copy(updatedAt = now) else it })
    }

    suspend fun rename(context: Context, id: String, name: String) {
        save(context, all(context).map { if (it.id == id) it.copy(name = name) else it })
    }

    /**
     * 一覧から外す。**プロジェクトごとの持ち物の片付けは [ProjectData.remove] が行う。**
     * ここを直接呼ぶのは [ProjectData.remove] からだけにする（09 章 §5 #1）。
     */
    suspend fun remove(context: Context, id: String) {
        save(context, all(context).filterNot { it.id == id })
    }
}
