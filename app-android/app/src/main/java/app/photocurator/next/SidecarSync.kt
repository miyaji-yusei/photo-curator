package app.photocurator.next

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.Job
import kotlinx.coroutines.async
import uniffi.photo_curator_core.ClashReason
import uniffi.photo_curator_core.Judgement
import uniffi.photo_curator_core.MergeMode
import uniffi.photo_curator_core.MergePreview
import uniffi.photo_curator_core.PairOverride
import uniffi.photo_curator_core.ProgressOrder
import uniffi.photo_curator_core.SeenRecord
import uniffi.photo_curator_core.Session
import uniffi.photo_curator_core.SettledReason
import uniffi.photo_curator_core.Sidecar
import uniffi.photo_curator_core.SidecarPhoto
import uniffi.photo_curator_core.SidecarPlan
import uniffi.photo_curator_core.SidecarProgress
import uniffi.photo_curator_core.SidecarSessions
import uniffi.photo_curator_core.isUntouched
import uniffi.photo_curator_core.judgementKey
import uniffi.photo_curator_core.mergeJudgements
import uniffi.photo_curator_core.sessionFromRatings
import uniffi.photo_curator_core.sidecarFromJson
import uniffi.photo_curator_core.sidecarJudgement
import uniffi.photo_curator_core.sidecarKeyCoverage
import uniffi.photo_curator_core.sidecarKeysFromFolder
import uniffi.photo_curator_core.sidecarKeysToFolder
import uniffi.photo_curator_core.sidecarNormalizeKeys
import uniffi.photo_curator_core.sidecarPlan
import uniffi.photo_curator_core.sidecarSeen
import uniffi.photo_curator_core.sidecarStamp
import uniffi.photo_curator_core.sidecarToJson
import uniffi.photo_curator_core.sidecarToken
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

// サイドカー（`.photo-curator/catalog.json`）の同期。**判断は core の `sidecarPlan` に従う**（U35）。
//
// ここは Android に依存しない（Context を持たない）。NAS・端末の控え・端末の選別状況は
// interface 越しに受け取るので、JVM の単体テストで偽物を渡して順番まで確かめられる。
// Android とのつなぎは Sidecar.kt。

/** NAS（SMB）への出し入れのうち、サイドカーが使うものだけ。パスは共有の根から `\` 区切り。 */
interface CatalogIO {
    /** 無ければ Ok(null)。つながらないときは Failed（無いことと区別する）。 */
    suspend fun read(path: String): SmbResult<ByteArray?>

    /** そのまま書く（あれば置き換える）。親フォルダが無ければ作る。一時ファイルと退避に使う。 */
    suspend fun write(path: String, bytes: ByteArray): SmbResult<Unit>

    /** 名前を変える。**先にあれば置き換える。** */
    suspend fun rename(from: String, to: String): SmbResult<Unit>

    /** 無いときだけ作る。先にあれば Ok(false)。親フォルダが無ければ作る。 */
    suspend fun createExclusive(path: String, bytes: ByteArray): SmbResult<Boolean>

    suspend fun delete(path: String): SmbResult<Unit>
}

/**
 * 端末の控え（プロジェクトごと）。
 *
 * [keyFromLocal] は古い控え（seenAt/seenBy/dirty）から移したばかりで、dirty が false だった印。
 * 比較キーをまだ持っていないので、次に端末の選別状況を読んだときにその比較キーで埋める。
 */
data class SeenState(
    val seen: SeenRecord,
    val detached: Boolean,
    val keyFromLocal: Boolean = false,
    /**
     * 端末の選別状況のファイルを**読めなかった**ことがある印（D2）。立っている間は、見た版を
     * 「まだ何も見ていない」として判断する（見た版のままでも、端末の分で NAS を自動で上書きしない。
     * 両方に判断があれば確認になる）。次に控えを保存したとき（取り込んだ・書いた・答えた）に下りる。
     */
    val localBroken: Boolean = false
)

/**
 * 端末の選別状況のファイルはあるのに読めなかった（形が合わない・読み込みの失敗）。
 * **「無い（未着手）」とは区別する。** 読めないまま判断すると、端末を空と見なして NAS を上書きしうる。
 */
class LocalUnreadable(val reason: String) : Exception(reason)

/** 端末の控えの置き場所。Android では SharedPreferences「sync」（`SyncState`）。 */
interface SeenStore {
    fun load(projectId: String): SeenState
    fun save(projectId: String, state: SeenState)
}

/** 端末の選別状況。**鍵は端末の形のまま**（Android は共有の根からの相対・`/`）。 */
data class LocalSnapshot(
    val session: Session?,
    val overrides: List<PairOverride>,
    val burstDistance: Int?,
    /** やり直しの世代。やり直すたびに新しい乱数になる。 */
    val epoch: String?
)

/** 端末の選別状況の出し入れ（プロジェクトごと）。 */
interface LocalState {
    /** 端末の保存の列が空になるまで待つ（最後の 1 組まで書き終えてから読む）。 */
    suspend fun flush()
    /** **ファイルはあるのに読めなければ [LocalUnreadable] を投げる**（空として返さない）。 */
    suspend fun read(): LocalSnapshot
    /** 置き換える。呼ぶ前に退避が済んでいること。 */
    suspend fun apply(snapshot: LocalSnapshot)
    /** 端末に控えを残す（`filesDir/aside/`）。中身はサイドカーと同じ形の JSON。 */
    suspend fun aside(json: String)
    /** 端末の写真の鍵（端末の形）。まだ分からなければ空。 */
    suspend fun photoKeys(): List<String>
    /** 混ぜた（D・E）あとのセッションの 1 組の枚数。 */
    fun groupSize(): Int
}

/** 同期する相手。 */
class SyncTarget(
    val projectId: String,
    /** 共有の根からの写真のフォルダ（`\` 区切り。空なら共有の根）。 */
    val folder: String,
    val io: CatalogIO,
    val local: LocalState
) {
    /** 端末の鍵の頭（フォルダ形式との変換に使う）。 */
    val prefix: String get() = folder.replace('\\', '/').trim('/')
}

/** この端末の名札。 */
data class Me(val id: String, val name: String)

/** どの契機で同期するか。 */
enum class SyncMode {
    /** プロジェクトを開いた・選別から戻った・選別を始める前。何でもする。 */
    Open,
    /** 背面へ回る・画面を離れる・ラウンドの終わり。**書くだけ**（取り込みと確認は次に開いたときへ）。 */
    Background,
    /** メニューの「NAS に保存」。判断は Open と同じ。 */
    Explicit
}

/** 同期の結果。画面はこれを 1 行にして出す。 */
sealed interface SyncOutcome {
    /** 何もしなかった（意味が同じ・変更なし・まだ何も無い）。[note] は言うことがあるときだけ。 */
    data class Settled(val note: String? = null) : SyncOutcome
    data class Pushed(val note: String) : SyncOutcome
    data class Pulled(val note: String) : SyncOutcome
    /** 両方とも着手していて中身が違う。**人に 5 択で選ばせる。** */
    data class Asking(val clash: SidecarClash) : SyncOutcome
    /** つなげない・読めない・書けない。**選別は止めない。** */
    data class Blocked(val reason: String) : SyncOutcome
    /** 背面への移動では取り込みも確認もしない。次に開いたときへ回した。 */
    data object Deferred : SyncOutcome
}

/** 食い違い（Clash）の中身。ダイアログに出す。 */
data class SidecarClash(
    /** ダイアログを出したときの NAS の版。**答えを実行する前に、変わっていないことを確かめる。** */
    val token: String,
    /** NAS の版（フォルダ形式の鍵）。 */
    val theirs: Sidecar,
    val mine: SidecarProgress,
    val theirsProgress: SidecarProgress,
    val order: ProgressOrder,
    val reason: ClashReason,
    val preview: MergePreview
)

/** ダイアログの 5 択（設計書 §4.6.1）。 */
enum class ClashChoice {
    /** A: サイドカーから取り込む。 */
    TakeTheirs,
    /** B: この端末の状況を残す（NAS に触らず切り離す。自動で書かない）。 */
    KeepMine,
    /** C: この端末の状況をサイドカーに書き込む。 */
    WriteMine,
    /** D: 両方で残した写真のみにする（積集合）。 */
    Intersection,
    /** E: どちらかで残した写真をすべて残す（和集合）。 */
    Union
}

/**
 * サイドカーの同期。**プロジェクトごとに 1 本の列**で、確認・書き込み・取り込み・答えの実行を並べる。
 *
 * - 戻ったときの確認が、自分の書き込み（画面を離れたときの push）を追い越さない（設計書 §2.3）
 * - 「変更があるか」は印ではなく「見た版の比較キーと、今の比較キーが違うか」（core が決める）
 * - 書くときは楽観ロック: ロックファイル → 読んで見た版と同じか確かめる → 一時ファイル → rename
 *   → 読み戻して確かめる（設計書 §4.4）
 * - 置き換える前に、必ず退避する（端末は [LocalState.aside]、NAS は `catalog.<端末>.json`）
 */
class SidecarSync(
    private val store: SeenStore,
    private val me: () -> Me,
    /** アプリの寿命の scope。**画面が消えても書き終える。** */
    private val scope: CoroutineScope,
    private val now: () -> Long = { System.currentTimeMillis() },
    private val newId: () -> String = { java.util.UUID.randomUUID().toString().replace("-", "") },
    private val log: (String, Throwable?) -> Unit = { _, _ -> }
) {
    private val tails = HashMap<String, Job>()

    /**
     * 列に積む。**呼んだ順に必ず走る**（積むのは呼んだその場で、起動の速さに左右されない）。
     * 前のものが失敗しても次は走る。
     */
    private fun <T> enqueue(projectId: String, block: suspend () -> T): Deferred<T> =
        synchronized(tails) {
            val previous = tails[projectId]
            val job = scope.async {
                previous?.join()
                block()
            }
            tails[projectId] = job
            job
        }

    /** 開いたとき・戻ったとき・始める前・明示の保存。**列の前のもの（自分の書き込み）を待ってから読む。** */
    suspend fun check(target: SyncTarget, mode: SyncMode = SyncMode.Open): SyncOutcome =
        enqueue(target.projectId) { guarded { sync(target, mode) } }.await()

    /**
     * 区切り（画面を離れる・ラウンドの終わり・背面へ回る）で書く。**列に積むだけで待たない。**
     * 書くかどうかは core が中身で決める（変更が無ければ何もしない）。
     */
    fun pushIfChanged(target: SyncTarget): Deferred<SyncOutcome> =
        enqueue(target.projectId) { guarded { sync(target, SyncMode.Background) } }

    /** ダイアログの答えを実行する。**NAS がダイアログを出したときから変わっていれば、判定し直す。** */
    suspend fun resolve(target: SyncTarget, clash: SidecarClash, choice: ClashChoice): SyncOutcome =
        enqueue(target.projectId) { guarded { resolveNow(target, clash, choice) } }.await()

    /** 切り離し（B）のあとの「NAS に書き込む」。C と同じ（NAS の版は退避する）。 */
    suspend fun writeNow(target: SyncTarget): SyncOutcome =
        enqueue(target.projectId) { guarded { writeMine(target) } }.await()

    /** 「この端末の状況を残す」を選んだあとか（自動で書かない）。 */
    fun detached(projectId: String): Boolean = store.load(projectId).detached

    // ---- 中身 ----

    private suspend fun guarded(block: suspend () -> SyncOutcome): SyncOutcome = try {
        block()
    } catch (error: kotlinx.coroutines.CancellationException) {
        throw error
    } catch (error: LocalUnreadable) {
        log("端末の選別状況を読めなかった", error)
        SyncOutcome.Blocked(LOCAL_UNREADABLE + error.reason + LOCAL_UNREADABLE_TAIL)
    } catch (error: Exception) {
        log("サイドカーの同期に失敗した", error)
        SyncOutcome.Blocked("NAS の記録を同期できませんでした: " + (error.message ?: error.javaClass.simpleName))
    }

    private sealed interface Remote {
        /** [sidecar] は鍵をフォルダ形式にそろえたもの。[raw] は読んだままのバイト列（退避に使う）。 */
        data class Read(val sidecar: Sidecar?, val raw: ByteArray?) : Remote
        data class Bad(val reason: String) : Remote
    }

    private suspend fun readRemote(target: SyncTarget): Remote =
        when (val answer = target.io.read(catalogPath(target.folder))) {
            is SmbResult.Failed -> Remote.Bad(answer.reason)
            is SmbResult.Ok -> {
                val bytes = answer.value
                if (bytes == null) {
                    Remote.Read(null, null)
                } else {
                    // **壊れていても上書きしない。** 読めないことにして人に伝える。
                    val parsed = try {
                        sidecarFromJson(String(bytes, Charsets.UTF_8))
                    } catch (error: Exception) {
                        null
                    }
                    if (parsed == null) {
                        Remote.Bad("NAS の記録（catalog.json）を読めませんでした（壊れている可能性）")
                    } else {
                        Remote.Read(sidecarNormalizeKeys(parsed, target.folder), bytes)
                    }
                }
            }
        }

    /**
     * 端末の選別状況を読む。**読めなければ印を立てて投げる**（判断も書き込みもしない。D2）。
     * 印が立っている間は、見た版を「まだ何も見ていない」として判断する（[planSeen]）。
     */
    private suspend fun readLocal(target: SyncTarget): LocalSnapshot = try {
        target.local.read()
    } catch (error: LocalUnreadable) {
        store.save(target.projectId, store.load(target.projectId).copy(localBroken = true))
        throw error
    }

    /**
     * 判断に渡す見た版。端末を読めなかったことがあれば「まだ何も見ていない」にする。
     * 見た版のままでも端末の分で NAS を自動で上書きせず、両方に判断があれば確認になる
     * （端末が空なら取り込む・NAS が未着手なら退避して書く、は変わらない）。
     */
    private fun planSeen(state: SeenState): SeenRecord =
        if (state.localBroken) SeenRecord("", "", null) else state.seen

    /** 端末の選別状況を、サイドカーの形（フォルダ形式の鍵）にする。 */
    private fun folderSidecar(target: SyncTarget, snapshot: LocalSnapshot): Sidecar {
        val who = me()
        val session = snapshot.session
        val device = Sidecar(
            version = 2,
            updatedAt = now(),
            updatedBy = who.id,
            updatedByName = who.name,
            // Android は星をセッションの中にしか持たない。
            photos = session?.ratings?.mapValues { SidecarPhoto(it.value) } ?: emptyMap(),
            burstOverrides = snapshot.overrides,
            sessions = SidecarSessions(session),
            burstDistance = snapshot.burstDistance?.takeIf { it >= 0 }?.toUInt(),
            writeId = null,
            basedOn = null,
            lineage = null,
            epoch = snapshot.epoch,
            keyBase = null,
            progress = null
        )
        return sidecarKeysToFolder(device, target.prefix)
    }

    /** 古い控えから移したばかりなら、今の端末の比較キーで埋める（dirty=false＝見た版と同じ）。 */
    private fun seenOf(projectId: String, judgement: Judgement): SeenState {
        val state = store.load(projectId)
        if (!state.keyFromLocal) return state
        val filled = state.copy(seen = state.seen.copy(key = judgementKey(judgement)), keyFromLocal = false)
        store.save(projectId, filled)
        return filled
    }

    private suspend fun sync(target: SyncTarget, mode: SyncMode): SyncOutcome {
        target.local.flush()
        var attempts = 0
        while (true) {
            val remote = when (val read = readRemote(target)) {
                is Remote.Bad -> return SyncOutcome.Blocked(read.reason)
                is Remote.Read -> read
            }
            val snapshot = readLocal(target)
            val mine = folderSidecar(target, snapshot)
            val judgement = sidecarJudgement(mine)
            val state = seenOf(target.projectId, judgement)
            val plan = sidecarPlan(planSeen(state), judgement, remote.sidecar, true, state.detached)
            when (plan) {
                is SidecarPlan.Settled -> {
                    plan.seen?.let {
                        // 意味が同じなら、切り離しも解く（もう食い違っていない）。
                        val stillDetached = state.detached && plan.reason != SettledReason.SAME
                        store.save(target.projectId, SeenState(it, stillDetached))
                    }
                    return SyncOutcome.Settled(settledNote(plan.reason))
                }
                is SidecarPlan.Push -> {
                    when (val pushed = push(target, mine, judgement, plan.expected, plan.asideTheirs)) {
                        is Pushed.Done -> return SyncOutcome.Pushed(pushed.note)
                        is Pushed.Failed -> return SyncOutcome.Blocked(pushed.reason)
                        is Pushed.Retry -> {
                            // 読んでから書くまでのあいだに、ほかの端末が書いた。**判定し直す。**
                            attempts += 1
                            if (attempts >= MAX_ATTEMPTS) {
                                return SyncOutcome.Blocked("NAS の記録が書いているあいだに変わりました。あとでもう一度試します")
                            }
                        }
                    }
                }
                is SidecarPlan.Pull -> {
                    if (mode == SyncMode.Background) return SyncOutcome.Deferred
                    return pull(target, plan.theirs, plan.asideMine, plan.seen, mine, judgement)
                }
                is SidecarPlan.Clash -> {
                    if (mode == SyncMode.Background) return SyncOutcome.Deferred
                    return SyncOutcome.Asking(
                        SidecarClash(
                            token = sidecarToken(plan.theirs),
                            theirs = plan.theirs,
                            mine = plan.mineProgress,
                            theirsProgress = plan.theirsProgress,
                            order = plan.order,
                            reason = plan.reason,
                            preview = plan.preview
                        )
                    )
                }
            }
        }
    }

    private fun settledNote(reason: SettledReason): String? = when (reason) {
        SettledReason.DETACHED -> "この端末だけの結果です（NAS とは別）"
        SettledReason.READ_ONLY -> "NAS に書けない共有です。この端末だけの結果です"
        SettledReason.NEWER_VERSION -> "新しい版のアプリが書いた記録です。この端末からは書きません"
        // 意味が同じ・変更なし・まだ何も無い → 何も言わない（ユーザーの決定）。
        else -> null
    }

    private sealed interface Pushed {
        data class Done(val note: String) : Pushed
        data class Failed(val reason: String) : Pushed
        data object Retry : Pushed
    }

    private sealed interface Lock {
        data object Taken : Lock
        data object Busy : Lock
        data class Failed(val reason: String) : Lock
    }

    /** 書く間だけ、ほかの端末と取り合わないための印。60 秒より古いものは捨てて取り直す。 */
    private suspend fun lock(target: SyncTarget): Lock {
        val path = lockPath(target.folder)
        val who = me()
        val body = "{\"device\":\"" + escape(who.id) + "\",\"name\":\"" + escape(who.name) +
            "\",\"at\":" + now() + "}"
        repeat(2) {
            when (val made = target.io.createExclusive(path, body.toByteArray(Charsets.UTF_8))) {
                is SmbResult.Failed -> return Lock.Failed(made.reason)
                is SmbResult.Ok -> if (made.value) return Lock.Taken
            }
            // 先にある。古ければ（書いた端末が途中で止まった）捨てて取り直す。
            val existing = target.io.read(path)
            val text = (existing as? SmbResult.Ok)?.value?.let { String(it, Charsets.UTF_8) }
            val at = text?.let { LOCK_AT.find(it)?.groupValues?.get(1)?.toLongOrNull() }
            val stale = text == null || at == null || now() - at > LOCK_TTL_MS
            if (!stale) return Lock.Busy
            target.io.delete(path)
        }
        return Lock.Busy
    }

    private suspend fun unlock(target: SyncTarget) {
        try {
            target.io.delete(lockPath(target.folder))
        } catch (error: Exception) {
            log("ロックを消せなかった", error)
        }
    }

    /**
     * 書く（楽観ロック）。**読んだ版が [expected] のままのときだけ置き換える。**
     *
     * 控え（seen）の比較キーは、書いた写し（[judgement]）から作る。書いている間に端末で
     * 増えた判断は比較キーが変わるので、次の同期で「変更あり」として残る。
     */
    private suspend fun push(
        target: SyncTarget,
        mine: Sidecar,
        judgement: Judgement,
        expected: String?,
        asideTheirs: Boolean
    ): Pushed {
        when (val taken = lock(target)) {
            is Lock.Failed -> return Pushed.Failed(taken.reason)
            is Lock.Busy -> return Pushed.Failed("ほかの端末が NAS に書いています。あとでもう一度試します")
            is Lock.Taken -> Unit
        }
        try {
            val current = when (val read = readRemote(target)) {
                is Remote.Bad -> return Pushed.Failed(read.reason)
                is Remote.Read -> read
            }
            val token = current.sidecar?.let { sidecarToken(it) }
            if (token != expected) return Pushed.Retry

            var aside: String? = null
            if (asideTheirs && current.sidecar != null && current.raw != null) {
                val owner = tag(current.sidecar.updatedBy)
                // **退避に失敗したら上書きしない。** 消してしまうより、次に持ち越す。
                when (val kept = target.io.write(asidePath(target.folder, owner), current.raw)) {
                    is SmbResult.Failed -> return Pushed.Failed(kept.reason)
                    is SmbResult.Ok -> aside = "catalog.$owner.json"
                }
            }

            val who = me()
            val writeId = newId()
            val stamped = sidecarStamp(
                mine.copy(updatedAt = now(), updatedBy = who.id, updatedByName = who.name),
                writeId,
                current.sidecar
            )
            val bytes = sidecarToJson(stamped).toByteArray(Charsets.UTF_8)
            // **一時ファイルに書いてから置き換える。** 途中で止まっても catalog.json は前のまま。
            val temporary = temporaryPath(target.folder, writeId)
            when (val written = target.io.write(temporary, bytes)) {
                is SmbResult.Failed -> {
                    target.io.delete(temporary)
                    return Pushed.Failed(written.reason)
                }
                is SmbResult.Ok -> Unit
            }
            when (val moved = target.io.rename(temporary, catalogPath(target.folder))) {
                is SmbResult.Failed -> {
                    target.io.delete(temporary)
                    return Pushed.Failed(moved.reason)
                }
                is SmbResult.Ok -> Unit
            }
            // 読み戻して確かめる（ロックを見ない古いアプリが同時に書いた場合）。
            val back = readRemote(target)
            if (back !is Remote.Read || back.sidecar?.writeId != writeId) return Pushed.Retry

            store.save(
                target.projectId,
                SeenState(SeenRecord(writeId, judgementKey(judgement), stamped.epoch), detached = false)
            )
            return Pushed.Done(
                "この端末の結果を NAS に保存しました" +
                    (aside?.let { "（NAS にあった記録は $it に残しました）" } ?: "")
            )
        } finally {
            unlock(target)
        }
    }

    /**
     * 取り込む。**先に端末の分を退避する**（端末に。[asideMine] なら NAS にも）。
     * 控えの比較キーは、取り込んだあとの端末の選別状況から作る。
     */
    private suspend fun pull(
        target: SyncTarget,
        theirs: Sidecar,
        asideMine: Boolean,
        seen: SeenRecord,
        mine: Sidecar,
        judgement: Judgement
    ): SyncOutcome {
        // 写真の場所が違う記録（別のフォルダ・古い形）は取り込まない（設計書 §4.8）。
        val keys = target.local.photoKeys().map { toFolderKey(target, it) }
        if (keys.isNotEmpty()) {
            val coverage = sidecarKeyCoverage(theirs, keys)
            if (coverage.total > 0u && coverage.matched * 2u < coverage.total) {
                return SyncOutcome.Blocked(
                    "NAS の記録は写真の場所が違うようです（一致 ${coverage.matched}/${coverage.total}）。取り込みませんでした"
                )
            }
        }
        val hadSomething = !isUntouched(judgement)
        if (hadSomething) {
            val json = sidecarToJson(mine)
            target.local.aside(json)
            if (asideMine) {
                val kept = target.io.write(asidePath(target.folder, tag(me().id)), json.toByteArray(Charsets.UTF_8))
                if (kept is SmbResult.Failed) log("NAS に端末の分を退避できなかった: " + kept.reason, null)
            }
        }
        apply(target, theirs)
        val after = sidecarJudgement(folderSidecar(target, readLocal(target)))
        store.save(target.projectId, SeenState(seen.copy(key = judgementKey(after)), detached = false))
        return SyncOutcome.Pulled(nameOf(theirs) + " の記録から続きを取り込みました")
    }

    /** NAS の版（フォルダ形式）で端末を置き換える。 */
    private suspend fun apply(target: SyncTarget, theirs: Sidecar) {
        val device = sidecarKeysFromFolder(theirs, target.prefix, "/")
        var session = device.sessions.tournament
        if (session == null) {
            // セッションが無く星だけある版（PC の古い形など）。**星を落とさない**よう、
            // その星の「完了した状態」にする（続きは結果画面の「もう一度選別する」）。
            val stars = device.photos.mapValues { it.value.rating }.filterValues { it > 0 }
            if (stars.isNotEmpty()) {
                session = sessionFromRatings(stars, 1u, 0, target.local.groupSize().toUInt(), emptyMap())
            }
        }
        target.local.apply(
            LocalSnapshot(
                session = session,
                overrides = device.burstOverrides,
                burstDistance = device.burstDistance?.toInt(),
                // やり直しの世代も NAS の版にそろえる（そろえないと、次に「端末がやり直した」と読む）。
                epoch = device.epoch
            )
        )
    }

    private suspend fun resolveNow(target: SyncTarget, clash: SidecarClash, choice: ClashChoice): SyncOutcome {
        target.local.flush()
        val remote = when (val read = readRemote(target)) {
            is Remote.Bad -> return SyncOutcome.Blocked(read.reason)
            is Remote.Read -> read
        }
        val current = remote.sidecar
        // ダイアログを出したあとで NAS が変わった → 選び直してもらう。
        if (current == null || sidecarToken(current) != clash.token) return sync(target, SyncMode.Open)

        val snapshot = readLocal(target)
        val mine = folderSidecar(target, snapshot)
        val judgement = sidecarJudgement(mine)
        return when (choice) {
            ClashChoice.TakeTheirs ->
                pull(target, current, true, sidecarSeen(current), mine, judgement)

            ClashChoice.KeepMine -> {
                // 控えは NAS の版（比較キーも NAS の中身）。端末とは違うので「変更あり」のまま、
                // 切り離しの間は書かない。NAS がまた変われば聞き直す。
                store.save(target.projectId, SeenState(sidecarSeen(current), detached = true))
                SyncOutcome.Settled("この端末の状況を残しました。NAS には書きません（この端末だけの結果）")
            }

            ClashChoice.WriteMine -> outcomeOf(push(target, mine, judgement, clash.token, asideTheirs = true))

            ClashChoice.Intersection, ClashChoice.Union -> {
                val mode = if (choice == ClashChoice.Union) MergeMode.UNION else MergeMode.INTERSECTION
                // 元の 2 つは両方退避する（端末の分は端末と NAS、NAS の分は書くときに NAS）。
                val json = sidecarToJson(mine)
                target.local.aside(json)
                val kept = target.io.write(asidePath(target.folder, tag(me().id)), json.toByteArray(Charsets.UTF_8))
                if (kept is SmbResult.Failed) return SyncOutcome.Blocked(kept.reason)

                val merged = mergeJudgements(
                    judgement, sidecarJudgement(current), mode, target.local.groupSize().toUInt(), newId()
                )
                val folderShape = mine.copy(
                    photos = merged.ratings.mapValues { SidecarPhoto(it.value) },
                    burstOverrides = merged.overrides,
                    sessions = SidecarSessions(merged.session),
                    burstDistance = merged.burstDistance,
                    epoch = merged.epoch,
                    keyBase = "folder"
                )
                val device = sidecarKeysFromFolder(folderShape, target.prefix, "/")
                target.local.apply(
                    LocalSnapshot(
                        session = device.sessions.tournament,
                        overrides = device.burstOverrides,
                        burstDistance = device.burstDistance?.toInt(),
                        epoch = device.epoch
                    )
                )
                val after = folderSidecar(target, readLocal(target))
                outcomeOf(push(target, after, sidecarJudgement(after), clash.token, asideTheirs = true))
            }
        }
    }

    private suspend fun writeMine(target: SyncTarget): SyncOutcome {
        target.local.flush()
        val remote = when (val read = readRemote(target)) {
            is Remote.Bad -> return SyncOutcome.Blocked(read.reason)
            is Remote.Read -> read
        }
        val mine = folderSidecar(target, readLocal(target))
        val expected = remote.sidecar?.let { sidecarToken(it) }
        return outcomeOf(push(target, mine, sidecarJudgement(mine), expected, asideTheirs = remote.sidecar != null))
    }

    private suspend fun outcomeOf(pushed: Pushed): SyncOutcome = when (pushed) {
        is Pushed.Done -> SyncOutcome.Pushed(pushed.note)
        is Pushed.Failed -> SyncOutcome.Blocked(pushed.reason)
        is Pushed.Retry -> SyncOutcome.Blocked("NAS の記録が書いているあいだに変わりました。もう一度開き直してください")
    }

    private fun toFolderKey(target: SyncTarget, key: String): String {
        val slash = key.replace('\\', '/')
        val head = target.prefix
        return if (head.isNotEmpty() && slash.startsWith("$head/")) slash.substring(head.length + 1) else slash
    }

    companion object {
        private const val MAX_ATTEMPTS = 3
        const val LOCK_TTL_MS = 60_000L
        private val LOCK_AT = Regex("\"at\"\\s*:\\s*(\\d+)")

        const val LOCAL_UNREADABLE = "この端末の選別の記録を読めませんでした: "
        const val LOCAL_UNREADABLE_TAIL = "。NAS の記録は変えていません（この端末の分で NAS を自動で上書きしません）"

        private fun dir(folder: String): String {
            val base = folder.trim('\\', '/').replace('/', '\\')
            return if (base.isEmpty()) ".photo-curator" else "$base\\.photo-curator"
        }

        /** 写真のフォルダの直下。**増やすのはこの 1 ファイルだけ**（書く間のロックと一時ファイルを除く）。 */
        fun catalogPath(folder: String) = dir(folder) + "\\catalog.json"
        fun lockPath(folder: String) = dir(folder) + "\\catalog.lock"
        /** 譲った方・置き換えた方を残す先。**黙って消さない。** */
        fun asidePath(folder: String, device: String) = dir(folder) + "\\catalog.$device.json"
        fun temporaryPath(folder: String, writeId: String) = dir(folder) + "\\.catalog.$writeId.tmp"

        /** 退避のファイル名に使う端末の印（12 文字。ファイル名に使えない文字は落とす）。 */
        fun tag(device: String): String =
            device.filter { it.isLetterOrDigit() || it == '-' || it == '_' }.take(12).ifEmpty { "unknown" }

        private fun escape(text: String) = text.replace("\\", "\\\\").replace("\"", "\\\"")

        fun nameOf(sidecar: Sidecar): String = sidecar.updatedByName.ifBlank { "別の端末" }

        // ---- ダイアログの文言（設計書 §4.6。PC・Web と同じにそろえる） ----

        const val CLASH_TITLE = "NAS の記録と、この端末の記録が違います"
        const val CLASH_FOOTNOTE = "どれを選んでも、元の 2 つは消しません。NAS の .photo-curator に " +
            "catalog.＜端末＞.json として残し、この端末にも控えます"
        const val CHOICE_TAKE_THEIRS = "サイドカーから取り込む"
        const val CHOICE_KEEP_MINE = "この端末の状況を残す"
        const val CHOICE_WRITE_MINE = "この端末の状況をサイドカーに書き込む"
        const val CHOICE_INTERSECTION = "両方で残した写真のみにする"
        const val CHOICE_UNION = "どちらかで残した写真をすべて残す"
        const val AHEAD_MARK = "こちらが進んでいます"

        /** 1 行の要約（例「★1 以上 120 枚・ROUND 1 の途中（42 組決定・278 枚残り）・手直し 3 か所」）。 */
        fun describe(progress: SidecarProgress): String {
            if (!progress.started) return "選別なし（未着手）"
            val parts = ArrayList<String>()
            parts += "★1 以上 ${progress.starred} 枚"
            parts += if (progress.finished) {
                "ROUND ${progress.round} 完了"
            } else {
                "ROUND ${progress.round} の途中（${progress.decided} 組決定・${progress.remaining} 枚残り）"
            }
            if (progress.overrides > 0u) parts += "手直し ${progress.overrides} か所"
            if (progress.learned) parts += "境目を学習済み"
            return parts.joinToString("・")
        }

        /** 「NAS（DESKTOP-ABC・10 月 1 日 22:10）」。**日時には必ず端末名を付ける。** */
        fun theirsLabel(theirs: Sidecar): String {
            val at = if (theirs.updatedAt > 0) {
                "・" + SimpleDateFormat("M 月 d 日 H:mm", Locale.JAPAN).format(Date(theirs.updatedAt))
            } else ""
            return "NAS（" + nameOf(theirs) + at + "）"
        }

        fun mineLabel(name: String) = "この端末（$name）"

        /** 理由の 1 行。 */
        fun reasonLine(clash: SidecarClash): String = when (clash.reason) {
            ClashReason.DIVERGED -> "どちらでも選別が進んでいて、中身が違います" + when (clash.order) {
                ProgressOrder.AHEAD -> "。この端末の方が進んでいます"
                ProgressOrder.BEHIND -> "。NAS の方が進んでいます"
                else -> ""
            }
            ClashReason.THEIRS_RESTARTED -> nameOf(clash.theirs) + " で最初からやり直されています"
            ClashReason.MINE_RESTARTED -> "この端末で最初からやり直しています"
            ClashReason.EXTRAS_CONFLICT -> "★と選別の進みは同じで、連写のまとまりの手直しか学習した境目が違います"
        }

        /** D・E を出すか。**片方の★1 以上が 0 枚なら出さない**（A か C と同じになる・全部消える）。 */
        fun canMerge(preview: MergePreview): Boolean =
            preview.mineStarred > 0u && preview.theirsStarred > 0u

        fun intersectionLine(preview: MergePreview) =
            "★1 以上が ${preview.intersectionStarred} 枚になります（この端末 ${preview.mineStarred}・NAS ${preview.theirsStarred}）"

        fun unionLine(preview: MergePreview) = "★1 以上が ${preview.unionStarred} 枚になります"

        /** 混ぜるとセッションが終わることの注意（途中のときだけ）。 */
        fun midRoundLine(preview: MergePreview): String? = if (!preview.midRound) null else
            "途中の ROUND は終わりにします。まだ見ていない ${preview.undecided} 枚は今の★のままになります。" +
                "続きは結果画面の『もう一度選別する』から"
    }
}
