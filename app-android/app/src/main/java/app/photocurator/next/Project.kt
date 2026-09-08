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
 * 出所。**人の言葉（label）と、機械の指し先を分けて持つ。**
 * 生のパスや ID は「技術情報」にだけ出す。
 */
data class Source(
    /** "album"（この端末）／"nas"（SMB）。 */
    val kind: String,
    /** 人に見せる言い方。「この端末・アルバム「Camera」」など。 */
    val label: String,
    /** kind ごとの指し先。album なら bucket id。 */
    val key: String
) {
    /** 技術情報に出す生の値。**普段は見せない。** */
    val technical: String get() = "$kind:$key"
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

    suspend fun all(context: Context): List<Project> = withContext(Dispatchers.IO) {
        val target = file(context)
        if (!target.exists()) return@withContext emptyList()
        try {
            val array = org.json.JSONArray(target.readText())
            (0 until array.length()).map { at ->
                val entry = array.getJSONObject(at)
                val source = entry.getJSONObject("source")
                Project(
                    id = entry.getString("id"),
                    name = entry.getString("name"),
                    source = Source(
                        kind = source.getString("kind"),
                        label = source.getString("label"),
                        key = source.getString("key")
                    ),
                    createdAt = entry.getLong("created"),
                    updatedAt = entry.getLong("updated")
                )
            // **更新順。** 2 回目以降は続きから始めることの方が多い。
            }.sortedByDescending { it.updatedAt }
        } catch (error: Exception) {
            Log.w(TAG, "プロジェクトを読めなかった", error)
            emptyList()
        }
    }

    suspend fun save(context: Context, projects: List<Project>) = withContext(Dispatchers.IO) {
        try {
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
                                .put("kind", project.source.kind)
                                .put("label", project.source.label)
                                .put("key", project.source.key)
                        )
                )
            }
            val target = file(context)
            val temporary = File(target.parentFile, "${target.name}.writing")
            temporary.writeText(array.toString())
            if (!temporary.renameTo(target)) {
                temporary.copyTo(target, overwrite = true)
                temporary.delete()
            }
            Unit
        } catch (error: Exception) {
            Log.w(TAG, "プロジェクトを保存できなかった", error)
        }
    }

    suspend fun add(context: Context, name: String, source: Source): Project {
        val now = System.currentTimeMillis()
        val project = Project("p$now", name, source, now, now)
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
     * 消す。**選別の結果も一緒に消える。写真そのものには触らない。**
     * 指紋は写真についての事実なので残す（別のプロジェクトでも使える）。
     */
    suspend fun remove(context: Context, id: String) {
        save(context, all(context).filterNot { it.id == id })
        // **プロジェクトごとのものは全部消す。** 星・手直し・学習した基準。
        // 消し忘れると使われないファイルが端末に残り続ける（実際に残っていた）。
        Store.clear(context, id)
        Overrides.clear(context, id)
        Learning.forget(context, id)
        // **サイドカーは消さない。** 写真側の持ち物なので、端末の都合で消さない。
        SyncState.forget(context, id)
    }
}
