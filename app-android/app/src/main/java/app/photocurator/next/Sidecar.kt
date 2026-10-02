package app.photocurator.next

import android.content.Context
import android.util.Log
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File

/**
 * 写真のフォルダに置く、判断の控え（`.photo-curator/catalog.json`）。**端末を変えても選び直さないため。**
 *
 * 星は人が時間をかけて付けたもので、作り直せない。それが端末の中にしか
 * 無いと、端末を変えた・アプリを消した時点で消える。写真の隣に置けば、
 * **写真と判断が一緒に移動する。**
 *
 * 原本のフォルダに増やすのは `.photo-curator/catalog.json` の 1 つだけ
 * （設計 CON-3。書く間のロックと一時ファイル、退避の `catalog.<端末>.<時刻>.json`（端末ごとに最新 5 つ）を除く）。
 *
 * **判断は core の `sidecarPlan`**（PC・Web と同じ規則）。列・楽観ロック・退避は [SidecarSync]。
 * ここは Android とのつなぎ（NAS の接続・端末の保存場所・控え）だけを持つ（U35）。
 */
object Sidecar {
    private const val TAG = "Sidecar"

    /** 端末の退避（`filesDir/aside/`）をプロジェクトごとにいくつまで残すか（U44 D4 で 3 → 5）。 */
    private const val ASIDE_KEEP = 5

    /** 画面が消えても書き終えるように、アプリの寿命で動く。 */
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    @Volatile private var engine: SidecarSync? = null

    private fun engine(context: Context): SidecarSync {
        engine?.let { return it }
        val app = context.applicationContext
        return synchronized(this) {
            engine ?: SidecarSync(
                store = SyncState.store(app),
                me = { Me(Device.id(app), Device.name()) },
                scope = scope,
                log = { message, error -> Log.w(TAG, message, error) },
                settings = settingsStore(app)
            ).also { engine = it }
        }
    }

    fun supports(project: Project): Boolean = project.source.kind == SourceKind.Nas

    private fun target(context: Context, project: Project): SyncTarget {
        val app = context.applicationContext
        return SyncTarget(
            projectId = project.id,
            folder = project.source.folder,
            io = SmbCatalogIO(app, project.source.key.substringBefore("|")),
            local = DeviceLocal(app, project)
        )
    }

    /**
     * 開いたとき・選別から戻ったとき・選別を始める前に確かめる。**列の前の書き込みを待ってから読む。**
     * 何をしたか（書いた・取り込んだ・確認が要る・つなげない）を返す。
     */
    suspend fun check(context: Context, project: Project, mode: SyncMode = SyncMode.Open): SyncOutcome {
        if (!supports(project)) return SyncOutcome.Settled()
        if (mode == SyncMode.Open) tidyLocalOnce(context, project.id)
        return engine(context).check(target(context, project), mode)
    }

    /** 端末の書きかけを片付けたプロジェクト（アプリの起動ごとに 1 回）。 */
    private val tidiedLocal = java.util.concurrent.ConcurrentHashMap.newKeySet<String>()

    /**
     * 端末の保存が途中で落ちて残した `.writing` を片付ける（U44 D11）。このプロジェクトの
     * 選別の途中・手直し・退避のうち、1 時間より古いものだけ。失敗しても止めない。
     */
    private suspend fun tidyLocalOnce(context: Context, id: String) {
        if (!tidiedLocal.add(id)) return
        withContext(Dispatchers.IO) {
            val now = System.currentTimeMillis()
            val gone = cleanWriting(context.filesDir, listOf("session-$id.json.", "overrides-$id.json."), now) +
                cleanWriting(File(context.filesDir, "aside"), listOf("$id-"), now)
            if (gone.isNotEmpty()) Log.i(TAG, "書きかけを片付けた: " + gone.joinToString { it.name })
        }
    }

    /**
     * 区切り（画面を離れる・ラウンドの終わり・背面へ回る）で書く。**列に積むだけで待たない。**
     * 書くかどうかは中身で決まる（変更が無ければ何もしない）。取り込み・確認は次に開いたとき。
     */
    fun pushIfChanged(context: Context, project: Project) {
        if (!supports(project)) return
        val pending = engine(context).pushIfChanged(target(context, project))
        scope.launch {
            val answer = pending.await()
            if (answer is SyncOutcome.Blocked) Log.w(TAG, "サイドカーを書けなかった: " + answer.reason)
        }
    }

    /** メニューの「NAS に保存」。**開いたときと同じ判断**（確かめずに上書きしない）。 */
    suspend fun save(context: Context, project: Project): SyncOutcome =
        check(context, project, SyncMode.Explicit)

    /** 食い違いのダイアログの答え。 */
    suspend fun resolve(context: Context, project: Project, clash: SidecarClash, choice: ClashChoice): SyncOutcome =
        engine(context).resolve(target(context, project), clash, choice)

    /** 切り離し（「この端末の状況を残す」）のあとの「NAS に書き込む」。 */
    suspend fun writeNow(context: Context, project: Project): SyncOutcome =
        engine(context).writeNow(target(context, project))

    /** 「この端末の状況を残す」を選んだあとか。 */
    fun detached(context: Context, project: Project): Boolean =
        supports(project) && engine(context).detached(project.id)

    /**
     * 結果を 1 行にする。**黙って書かない、黙って失敗しない。** 何もしなかったときは null。
     * ほかの端末の設定を取り込んだら（U51）、そのお知らせを後ろに足す（確認のダイアログのときも出す）。
     */
    fun note(outcome: SyncOutcome): String? {
        val base = when (outcome) {
            is SyncOutcome.Settled -> outcome.note
            is SyncOutcome.Pushed -> outcome.note
            is SyncOutcome.Pulled -> outcome.note
            is SyncOutcome.Blocked -> outcome.reason
            is SyncOutcome.Asking, SyncOutcome.Deferred -> null
        }
        val adopted = outcome.settingsAdopted?.let { SidecarSync.settingsAdoptedNotice(it) } ?: return base
        return if (base == null) adopted else "$base。$adopted"
    }

    // ---- つなぎ ----

    /** プロジェクトの設定（U51）。`Prefs` の「同名の JPEG と RAW を 1 枚として扱う」と、切り替えた時刻。 */
    private fun settingsStore(context: Context): SettingsStore = object : SettingsStore {
        override fun pairRaw(projectId: String) = Prefs.pairRawSetting(context, projectId)
        override fun setPairRaw(projectId: String, enabled: Boolean, at: Long) =
            Prefs.setPairRawJpeg(context, projectId, enabled, at)
    }

    /** NAS（SMB）。**接続の情報は使うときに読む**（列に積むのはその場で、待たずに済ませるため）。 */
    private class SmbCatalogIO(private val context: Context, private val nasId: String) : CatalogIO {
        private var cached: Pair<Nas, String>? = null

        private suspend fun credentials(): Pair<Nas, String>? {
            cached?.let { return it }
            val nas = NasStore.all(context).firstOrNull { it.id == nasId } ?: return null
            val password = NasPasswords.password(context, nas) ?: return null
            return (nas to password).also { cached = it }
        }

        private suspend fun <T> with(work: suspend (Nas, String) -> SmbResult<T>): SmbResult<T> {
            val (nas, password) = credentials() ?: return SmbResult.Failed("NAS のパスワードが要ります")
            return work(nas, password)
        }

        override suspend fun read(path: String) = with { nas, password -> Smb.readIfExists(nas, password, path) }
        override suspend fun write(path: String, bytes: ByteArray) =
            with { nas, password -> Smb.writeDirect(nas, password, path, bytes) }
        override suspend fun rename(from: String, to: String) =
            with { nas, password -> Smb.rename(nas, password, from, to) }
        override suspend fun createExclusive(path: String, bytes: ByteArray) =
            with { nas, password -> Smb.createExclusive(nas, password, path, bytes) }
        override suspend fun delete(path: String) = with { nas, password -> Smb.delete(nas, password, path) }
        override suspend fun list(folder: String): SmbResult<List<CatalogEntry>> =
            when (val listed = with { nas, password -> Smb.listSidecar(nas, password, folder) }) {
                is SmbResult.Failed -> SmbResult.Failed(listed.reason)
                is SmbResult.Ok -> SmbResult.Ok(listed.value.map { CatalogEntry(it.first, it.second) })
            }
    }

    /** 端末の選別状況（星とセッション・手直し・学習した境目・やり直しの世代）。 */
    private class DeviceLocal(private val context: Context, private val project: Project) : LocalState {
        private val id get() = project.id

        override suspend fun flush() {
            // 選別の確定・手直しは Persist の列で書く。**最後の 1 組まで書き終えてから読む。**
            Persist.settle("session:$id")
            Persist.settle("overrides:$id")
        }

        override suspend fun read(): LocalSnapshot {
            // **「無い」と「読めなかった」を分ける**（D2）。読めなければ空として渡さない。
            val session = when (val read = Store.read(context, id)) {
                is Stored.Ok -> read.value
                is Stored.Broken -> throw LocalUnreadable(brokenNote("選別の途中", read))
            }
            val overrides = when (val read = Overrides.read(context, id)) {
                is Stored.Ok -> read.value
                is Stored.Broken -> throw LocalUnreadable(brokenNote("連写の手直し", read))
            }
            return LocalSnapshot(
                session = session,
                overrides = overrides,
                burstDistance = Learning.learned(context, id),
                epoch = SyncState.epoch(context, id)
            )
        }

        private fun brokenNote(what: String, read: Stored.Broken) =
            what + "（" + read.reason.take(60) + "）" +
                (read.keptAs?.let { "。元の中身は ${it.name} に残しました" } ?: "")

        override suspend fun apply(snapshot: LocalSnapshot) {
            snapshot.session?.let { Store.save(context, id, it) } ?: Store.clear(context, id)
            if (snapshot.overrides.isEmpty()) Overrides.clear(context, id)
            else Overrides.save(context, id, snapshot.overrides)
            snapshot.burstDistance?.let { Learning.save(context, id, it) } ?: Learning.forget(context, id)
            SyncState.setEpoch(context, id, snapshot.epoch)
        }

        override suspend fun aside(json: String) = withContext(Dispatchers.IO) {
            // 置き換える前の端末の選別状況。**書けなければ置き換えない**（例外のまま返す）。
            val dir = File(context.filesDir, "aside").apply { mkdirs() }
            // 名前は時刻（ミリ秒）。**前の退避を上書きしない**（同じ時刻があれば 1 つずらす）。
            var at = System.currentTimeMillis()
            while (File(dir, "$id-$at.json").exists()) at += 1
            File(dir, "$id-$at.json").writeAtomically { it.writeText(json) }
            // **最後の 5 つまで**残す（元に戻す手がかり）。片付けの失敗は止めない。
            try {
                dir.listFiles { file -> file.name.startsWith("$id-") && file.name.endsWith(".json") }
                    ?.sortedByDescending { it.name.removePrefix("$id-").removeSuffix(".json").toLongOrNull() ?: 0L }
                    ?.drop(ASIDE_KEEP)
                    ?.forEach { it.delete() }
            } catch (error: Exception) {
                Log.w(TAG, "古い退避を片付けられなかった: $id", error)
            }
            Unit
        }

        override suspend fun photoKeys(): List<String> =
            Listing.load(context, project.source.key)?.map { it.relativePath } ?: emptyList()

        override fun groupSize(): Int = Prefs.groupSize(context)
    }
}
