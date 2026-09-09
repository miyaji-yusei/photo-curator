package app.photocurator.next

import android.content.Context
import android.util.Log
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import uniffi.photo_curator_core.PairOverride
import uniffi.photo_curator_core.Session as CoreSession
import uniffi.photo_curator_core.sessionFromJson
import uniffi.photo_curator_core.sessionToJson

/**
 * 写真のフォルダに置く、判断の控え。**端末を変えても選び直さないため。**
 *
 * 星は人が時間をかけて付けたもので、作り直せない。それが端末の中にしか
 * 無いと、端末を変えた・アプリを消した時点で消える。写真の隣に置けば、
 * **写真と判断が一緒に移動する。**
 *
 * 原本のフォルダに増やすのは `.photo-curator/catalog.json` の 1 つだけ
 * （設計 CON-3）。サムネイルや表示用画像は置かない。数百 MB を NAS に
 * 書くことになるし、作り直せるものは端末で足りる。
 */
data class Catalog(
    val updatedAt: Long,
    /** どの端末が書いたか。**競合したときに人が判断できるように。** */
    val updatedBy: String,
    val updatedByName: String,
    /** 相対パス → 星。撮影時刻や指紋は端末側で作り直せるので、いまは星だけ。 */
    val ratings: Map<String, Int>,
    val overrides: List<PairOverride>,
    /** 選別の途中。**core の Session をそのまま。** */
    val session: CoreSession?,
    /** 学習した「見た目が近い」の境目。人が答えて決めたものなので一緒に運ぶ。 */
    val burstDistance: Int?
) {
    /** 食い違ったときに人が見比べる 1 行。 */
    fun summary(): String {
        val kept = ratings.values.count { it > 0 }
        val round = session?.let {
            "ROUND " + it.round + (if (it.finished) "（完了）" else " の途中")
        } ?: "選別なし"
        return "★1 以上 $kept 枚 · $round"
    }
}

/** 開いたときにどうするか。 */
sealed interface Sync {
    /** 何も起きていない。 */
    data object Settled : Sync

    /** 端末の方が進んでいる。**書けばよい。** */
    data object Push : Sync

    /** サイドカーの方が進んでいる。**取り込めばよい。** */
    data class Pull(val catalog: Catalog) : Sync

    /** 両方が進んでいる。**人に選ばせる。** */
    data class Clash(val catalog: Catalog) : Sync

    /** つなげない・読めない。**選別は止めない。** */
    data class Blocked(val reason: String) : Sync
}

object Sidecar {
    private const val TAG = "Sidecar"
    private const val VERSION = 1

    /**
     * 画面が消えても書けるように、アプリの寿命で動く。
     *
     * **書く契機は 4 つ**（設計 03）: ラウンドが終わったとき／背面へ回るとき／
     * プロジェクトを閉じるとき／明示の保存。どれも「端末側に変更があるとき
     * だけ」。無いのに書くと updatedAt が動き、次に開いたとき自分の書き込みを
     * 他人の変更と誤認する。
     */
    private val scope = kotlinx.coroutines.CoroutineScope(
        kotlinx.coroutines.SupervisorJob() + Dispatchers.IO
    )

    /**
     * 変更があれば書く。**無ければ何もしない。**
     * 画面が消えたあとでも走るので、戻るボタンや背面への移動から呼べる。
     */
    fun pushIfChanged(context: Context, project: Project) {
        if (!supports(project)) return
        if (!SyncState.changed(context, project.id)) return
        val app = context.applicationContext
        scope.launch {
            val failed = push(app, project)
            if (failed != null) Log.w(TAG, "サイドカーを書けなかった: " + failed)
        }
    }

    /** 写真のフォルダの直下。**増やすのはこの 1 ファイルだけ。** */
    private fun path(folder: String) = "$folder\\.photo-curator\\catalog.json"

    /** 譲らなかった方を残す先。**黙って上書きしない。** */
    private fun asidePath(folder: String, device: String) =
        "$folder\\.photo-curator\\catalog.$device.json"

    fun supports(project: Project): Boolean = project.source.kind == "nas"

    private fun nasId(project: Project) = project.source.key.substringBefore("|")
    private fun folder(project: Project) = project.source.folder

    // ---- 形 ----

    private fun encode(catalog: Catalog): ByteArray {
        val photos = JSONObject()
        for ((path, star) in catalog.ratings) {
            photos.put(path, JSONObject().put("rating", star))
        }
        val overrides = JSONArray()
        for (one in catalog.overrides) {
            overrides.put(
                JSONObject().put("l", one.left).put("r", one.right).put("d", one.decision)
            )
        }
        val sessions = JSONObject()
        catalog.session?.let { sessions.put("tournament", JSONObject(sessionToJson(it))) }
        val root = JSONObject()
            .put("version", VERSION)
            .put("updatedAt", catalog.updatedAt)
            .put("updatedBy", catalog.updatedBy)
            .put("updatedByName", catalog.updatedByName)
            .put("photos", photos)
            .put("burstOverrides", overrides)
            .put("sessions", sessions)
        catalog.burstDistance?.let { root.put("burstDistance", it) }
        return root.toString(2).toByteArray()
    }

    private fun decode(bytes: ByteArray): Catalog? = try {
        val root = JSONObject(String(bytes))
        val photos = root.optJSONObject("photos") ?: JSONObject()
        val ratings = HashMap<String, Int>()
        for (key in photos.keys()) {
            ratings[key] = photos.getJSONObject(key).optInt("rating", 0)
        }
        val overrides = ArrayList<PairOverride>()
        val array = root.optJSONArray("burstOverrides") ?: JSONArray()
        for (at in 0 until array.length()) {
            val one = array.getJSONObject(at)
            overrides += PairOverride(
                one.getString("l"), one.getString("r"), one.getString("d")
            )
        }
        val session = root.optJSONObject("sessions")?.optJSONObject("tournament")
            ?.let { sessionFromJson(it.toString()) }
        Catalog(
            updatedAt = root.optLong("updatedAt", 0L),
            updatedBy = root.optString("updatedBy", "?"),
            updatedByName = root.optString("updatedByName", "別の端末"),
            ratings = ratings,
            overrides = overrides,
            session = session,
            burstDistance = if (root.has("burstDistance")) root.getInt("burstDistance") else null
        )
    } catch (error: Exception) {
        // **壊れていても上書きしない。** 読めないことにして人に伝える。
        Log.w(TAG, "catalog.json を読めなかった", error)
        null
    }

    // ---- 出し入れ ----

    private suspend fun credentials(context: Context, project: Project): Pair<Nas, String>? {
        val nas = NasStore.all(context).firstOrNull { it.id == nasId(project) } ?: return null
        val password = Session.password(context, nas) ?: return null
        return nas to password
    }

    /**
     * 開いたときの判定。**時刻の大小では決めない。**
     *
     * 端末ごとに時計はずれるので、「新しい方」を選ぼうとすると狂った端末が
     * 常に勝つ。見るのは「**自分が最後に見た版と同じかどうか**」だけ。
     */
    suspend fun check(context: Context, project: Project): Sync = withContext(Dispatchers.IO) {
        if (!supports(project)) return@withContext Sync.Settled
        val (nas, password) = credentials(context, project)
            ?: return@withContext Sync.Blocked("NAS のパスワードが要ります")

        val answer = Smb.readIfExists(nas, password, path(folder(project)))
        val bytes = when (answer) {
            is SmbResult.Failed -> return@withContext Sync.Blocked(answer.reason)
            is SmbResult.Ok -> answer.value
        }

        val mineChanged = SyncState.changed(context, project.id)
        if (bytes == null) {
            // まだ無い。**端末に何かあるなら置きに行く。**
            return@withContext if (mineChanged) Sync.Push else Sync.Settled
        }
        val theirs = decode(bytes)
            ?: return@withContext Sync.Blocked("NAS の記録を読めませんでした（壊れている可能性）")

        val same = theirs.updatedAt == SyncState.seenAt(context, project.id) &&
            theirs.updatedBy == SyncState.seenBy(context, project.id)
        when {
            same && !mineChanged -> Sync.Settled
            same && mineChanged -> Sync.Push
            !same && !mineChanged -> Sync.Pull(theirs)
            else -> Sync.Clash(theirs)
        }
    }

    /** 端末の中身を集めて 1 つにする。 */
    private suspend fun mine(context: Context, project: Project): Catalog {
        val session = Store.load(context, project.id)
        return Catalog(
            updatedAt = System.currentTimeMillis(),
            updatedBy = Device.id(context),
            updatedByName = Device.name(),
            ratings = session?.ratings ?: emptyMap(),
            overrides = Overrides.load(context, project.id),
            session = session,
            burstDistance = Learning.learned(context, project.id)
        )
    }

    /**
     * 端末のものを NAS へ置く。**書けたときだけ「見た版」を進める。**
     *
     * 進め忘れると、次に開いたとき自分が書いたものを他人の変更と誤認する。
     * 逆に失敗したのに進めると、書けていない内容を「共有済み」と思い込む。
     */
    suspend fun push(context: Context, project: Project): String? = withContext(Dispatchers.IO) {
        if (!supports(project)) return@withContext null
        val (nas, password) = credentials(context, project)
            ?: return@withContext "NAS のパスワードが要ります"
        val catalog = mine(context, project)
        val answer = Smb.write(nas, password, path(folder(project)), encode(catalog))
        when (answer) {
            is SmbResult.Failed -> answer.reason
            is SmbResult.Ok -> {
                SyncState.saw(context, project.id, catalog.updatedAt, catalog.updatedBy)
                null
            }
        }
    }

    /**
     * NAS のものを端末に取り込む。**端末側の判断は上書きされる。**
     * 呼ぶ前に、上書きしてよいことが決まっていること（Pull か、人が選んだ Clash）。
     */
    suspend fun adopt(context: Context, project: Project, catalog: Catalog) =
        withContext(Dispatchers.IO) {
            catalog.session?.let { Store.save(context, project.id, it) }
            Overrides.save(context, project.id, catalog.overrides)
            catalog.burstDistance?.let { Learning.save(context, project.id, it) }
            SyncState.saw(context, project.id, catalog.updatedAt, catalog.updatedBy)
            // 取り込んだ直後は**端末に未共有の変更が無い**状態。
            SyncState.clean(context, project.id)
        }

    /**
     * 端末のものを選んだとき。**譲られた方を捨てない。**
     * `catalog.<端末名>.json` に退避してから、端末のもので上書きする。
     */
    suspend fun keepMine(context: Context, project: Project, theirs: Catalog): String? =
        withContext(Dispatchers.IO) {
            val (nas, password) = credentials(context, project)
                ?: return@withContext "NAS のパスワードが要ります"
            val aside = asidePath(folder(project), theirs.updatedBy.take(12))
            // 退避に失敗したら**上書きしない。** 消してしまうより、次に持ち越す。
            when (val kept = Smb.write(nas, password, aside, encode(theirs))) {
                is SmbResult.Failed -> return@withContext kept.reason
                is SmbResult.Ok -> Unit
            }
            push(context, project)
        }
}
