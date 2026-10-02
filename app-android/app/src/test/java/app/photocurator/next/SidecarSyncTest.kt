package app.photocurator.next

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.photo_curator_core.Decision
import uniffi.photo_curator_core.SeenRecord
import uniffi.photo_curator_core.Session
import uniffi.photo_curator_core.Sidecar
import uniffi.photo_curator_core.SidecarPhoto
import uniffi.photo_curator_core.SidecarSessions
import uniffi.photo_curator_core.judgementEquivalent
import uniffi.photo_curator_core.judgementKey
import uniffi.photo_curator_core.sidecarFromJson
import uniffi.photo_curator_core.sidecarJudgement
import uniffi.photo_curator_core.sidecarKeysToFolder
import uniffi.photo_curator_core.sidecarNormalizeKeys
import uniffi.photo_curator_core.sidecarToJson
import java.io.IOException
import java.util.concurrent.atomic.AtomicInteger

/**
 * サイドカーの同期（U35）。設計書 §5 の PR-1 の再現テストと、5 択・早送り・切り離し。
 *
 * NAS は [FakeNas]（メモリ上。呼び出しを [CompletableDeferred] で止められる）、端末の控えは
 * [FakeSeen]、端末の選別状況は [FakeLocal]。判断は**本物の core**（PC 向けに組んだ同じ core）。
 */
class SidecarSyncTest {
    private val folder = "photo\\trip"
    private val catalog = SidecarSync.catalogPath(folder)
    private val photos = (1..12).map { "photo/trip/IMG_%03d.JPG".format(it) }
    private fun p(n: Int) = photos[n - 1]

    private val android = Me("android-0001", "Pixel 8")
    private val fold = Me("fold-0000002", "Fold")

    // ---- 偽物 ----

    class FakeNas : CatalogIO {
        val files = java.util.Collections.synchronizedMap(LinkedHashMap<String, ByteArray>())
        val reads = AtomicInteger()
        /** 書き込み（一時ファイル・退避）の直前に呼ぶ。止めたいときに使う。 */
        @Volatile var beforeWrite: (suspend (String) -> Unit)? = null
        /** catalog.json を読んだあと、返す前に呼ぶ（読んだ中身はもう決まっている）。 */
        @Volatile var afterRead: (suspend (String) -> Unit)? = null
        /** true を返した書き込みは、半分だけ書いて例外にする（途中で切れた）。 */
        @Volatile var breakWrite: (String) -> Boolean = { false }
        /** true を返した書き込み・作成は Failed にする（書けない）。 */
        @Volatile var refuse: (String) -> Boolean = { false }
        /** 更新時刻（NAS の時計）。書いた・作った・名前を変えたときに [nasNow] を入れる。無いものは 0（とても古い）。 */
        val mtimes = java.util.concurrent.ConcurrentHashMap<String, Long>()
        @Volatile var nasNow: Long = 0L

        override suspend fun list(folder: String): SmbResult<List<CatalogEntry>> {
            val head = folder + "\\"
            val names = synchronized(files) { files.keys.toList() }
            return SmbResult.Ok(
                names.filter { it.startsWith(head) && !it.substring(head.length).contains('\\') }
                    .map { CatalogEntry(it.substring(head.length), mtimes[it] ?: 0L) }
            )
        }

        /** どのファイルでも、読んだあと返す前に呼ぶ（読んだ中身はもう決まっている）。 */
        @Volatile var afterAnyRead: (suspend (String) -> Unit)? = null

        override suspend fun read(path: String): SmbResult<ByteArray?> {
            val bytes = files[path]
            if (path.endsWith("catalog.json")) {
                reads.incrementAndGet()
                afterRead?.invoke(path)
            }
            afterAnyRead?.invoke(path)
            return SmbResult.Ok(bytes)
        }

        override suspend fun write(path: String, bytes: ByteArray): SmbResult<Unit> {
            beforeWrite?.invoke(path)
            if (refuse(path)) return SmbResult.Failed("書けない: $path")
            if (breakWrite(path)) {
                files[path] = bytes.copyOf(bytes.size / 2)
                throw IOException("書いている途中で切れた")
            }
            files[path] = bytes
            mtimes[path] = nasNow
            return SmbResult.Ok(Unit)
        }

        override suspend fun rename(from: String, to: String): SmbResult<Unit> {
            val bytes = files.remove(from) ?: return SmbResult.Failed("無い: $from")
            files[to] = bytes
            mtimes.remove(from)
            mtimes[to] = nasNow
            return SmbResult.Ok(Unit)
        }

        override suspend fun createExclusive(path: String, bytes: ByteArray): SmbResult<Boolean> {
            if (refuse(path)) return SmbResult.Failed("書けない: $path")
            return synchronized(files) {
                if (files.containsKey(path)) SmbResult.Ok(false)
                else {
                    files[path] = bytes
                    mtimes[path] = nasNow
                    SmbResult.Ok(true)
                }
            }
        }

        override suspend fun delete(path: String): SmbResult<Unit> {
            files.remove(path)
            mtimes.remove(path)
            return SmbResult.Ok(Unit)
        }

        fun text(path: String): String? = files[path]?.let { String(it, Charsets.UTF_8) }
    }

    class FakeSeen : SeenStore {
        val states = HashMap<String, SeenState>()
        override fun load(projectId: String) =
            states[projectId] ?: SeenState(SeenRecord("", "", null), detached = false)

        override fun save(projectId: String, state: SeenState) {
            states[projectId] = state
        }
    }

    class FakeLocal(var snapshot: LocalSnapshot, private val keys: List<String> = emptyList()) : LocalState {
        val asides = ArrayList<String>()
        /** null でなければ、端末のファイルが読めない（形が合わない）ことにする。 */
        @Volatile var broken: String? = null
        override suspend fun flush() = Unit
        override suspend fun read(): LocalSnapshot {
            broken?.let { throw LocalUnreadable(it) }
            return snapshot
        }
        override suspend fun apply(snapshot: LocalSnapshot) {
            this.snapshot = snapshot
        }
        override suspend fun aside(json: String) {
            asides += json
        }
        override suspend fun photoKeys() = keys
        override fun groupSize() = 4
    }

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private var clock = 1_790_000_000_000L
    private val ids = AtomicInteger()

    private fun syncFor(who: Me, seen: FakeSeen) = SidecarSync(
        store = seen,
        me = { who },
        scope = scope,
        now = { clock++ },
        newId = { "w" + ids.incrementAndGet() + who.id.take(4) }
    )

    // ---- 選別状況の見本（端末の鍵） ----

    /** 「選別を開始」を押しただけ（1 手も進んでいない）。 */
    private fun untouched() = Session(
        groupSize = 4u, targetStar = 0, round = 1u,
        queue = photos.drop(4), current = photos.take(4), survivors = emptyList(),
        ratings = photos.associateWith { 0 }, members = emptyMap(), history = emptyList(),
        finished = false
    )

    /** ROUND 1 で 2 組決めた（★: 1・2・5）。 */
    private fun mineAdvanced() = Session(
        groupSize = 4u, targetStar = 0, round = 1u,
        queue = emptyList(), current = photos.drop(8), survivors = listOf(p(1), p(2), p(5)),
        ratings = photos.associateWith { 0 } + mapOf(p(1) to 1, p(2) to 1, p(5) to 1),
        members = emptyMap(),
        history = listOf(
            Decision(photos.subList(0, 4), listOf(p(1), p(2)), null, emptyMap()),
            Decision(photos.subList(4, 8), listOf(p(5)), null, emptyMap())
        ),
        finished = false
    )

    /** [mineAdvanced] からもう 1 組決めた（★: 9 も）。 */
    private fun mineFurther() = mineAdvanced().copy(
        current = emptyList(),
        survivors = listOf(p(1), p(2), p(5), p(9)),
        ratings = mineAdvanced().ratings + mapOf(p(9) to 1),
        history = mineAdvanced().history + Decision(photos.subList(8, 12), listOf(p(9)), null, emptyMap())
    )

    /** 別の端末で 1 組決めた（★: 3）。 */
    private fun theirsAdvanced() = Session(
        groupSize = 4u, targetStar = 0, round = 1u,
        queue = photos.drop(8), current = photos.subList(4, 8), survivors = listOf(p(3)),
        ratings = photos.associateWith { 0 } + mapOf(p(3) to 1),
        members = emptyMap(),
        history = listOf(Decision(photos.subList(0, 4), listOf(p(3)), null, emptyMap())),
        finished = false
    )

    private fun snapshot(session: Session?) = LocalSnapshot(session, emptyList(), null, null)

    private fun target(local: FakeLocal, nas: FakeNas) = SyncTarget("project-1", folder, nas, local)

    /** 端末の形の選別状況を、別の端末が書いたサイドカー（フォルダ形式の鍵）にする。 */
    private fun written(
        session: Session?,
        by: Me,
        writeId: String?,
        basedOn: String? = null,
        updatedAt: Long = 1_789_000_000_000L
    ): Sidecar {
        val device = Sidecar(
            version = if (writeId == null) 1 else 2,
            updatedAt = updatedAt, updatedBy = by.id, updatedByName = by.name,
            photos = session?.ratings?.mapValues { SidecarPhoto(it.value) } ?: emptyMap(),
            burstOverrides = emptyList(), sessions = SidecarSessions(session), burstDistance = null,
            writeId = writeId, basedOn = basedOn, lineage = basedOn?.let { listOf(it) }, epoch = null,
            keyBase = null, progress = null
        )
        val folderShape = sidecarKeysToFolder(device, "photo/trip")
        return if (writeId == null) folderShape.copy(keyBase = null) else folderShape
    }

    private fun put(nas: FakeNas, sidecar: Sidecar): ByteArray {
        val bytes = sidecarToJson(sidecar).toByteArray(Charsets.UTF_8)
        nas.files[catalog] = bytes
        return bytes
    }

    /** その端末の印の、NAS の時刻つきの退避（パス。新しい順）。 */
    private fun asides(nas: FakeNas, device: String): List<String> {
        val head = SidecarSync.sidecarDir(folder) + "\\"
        return synchronized(nas.files) { nas.files.keys.toList() }
            .filter { it.startsWith(head) && SidecarSync.isAsideOf(it.substring(head.length), device) }
            .sortedDescending()
    }

    /** その端末の印の、いちばん新しい退避の中身。 */
    private fun aside(nas: FakeNas, device: String): ByteArray? = asides(nas, device).firstOrNull()?.let { nas.files[it] }

    /** NAS の catalog.json の選別状況（鍵はフォルダ形式にそろえる）。 */
    private fun remote(nas: FakeNas): Sidecar =
        sidecarNormalizeKeys(sidecarFromJson(nas.text(catalog)!!)!!, folder)

    private fun localSidecar(local: FakeLocal) = sidecarKeysToFolder(
        Sidecar(
            2, 0, "", "", local.snapshot.session?.ratings?.mapValues { SidecarPhoto(it.value) } ?: emptyMap(),
            local.snapshot.overrides, SidecarSessions(local.snapshot.session),
            local.snapshot.burstDistance?.toUInt(), null, null, null, local.snapshot.epoch, null, null
        ),
        "photo/trip"
    )

    private fun sameJudgement(local: FakeLocal, sidecar: Sidecar) =
        judgementEquivalent(sidecarJudgement(localSidecar(local)), sidecarJudgement(sidecar))

    /** 端末がその版を見て、そのあと何も変えていない控え。 */
    private fun seenAt(sidecar: Sidecar) = SeenState(
        SeenRecord(
            sidecar.writeId ?: "legacy:${sidecar.updatedAt}:${sidecar.updatedBy}",
            judgementKey(sidecarJudgement(sidecar)),
            null
        ),
        detached = false
    )

    private fun <T> run(block: suspend CoroutineScope.() -> T): T = runBlocking { withTimeout(20_000) { block() } }

    // ---- テスト 1: タイムライン B（戻ったときの push と check の追い越し） ----

    @Test fun 戻ったときの確認は自分の書き込みを追い越さない() = run {
        val nas = FakeNas()
        val a1 = written(mineAdvanced(), android, "a1")
        put(nas, a1)
        val seen = FakeSeen().apply { save("project-1", seenAt(a1)) }
        // 端末は A1 からもう 1 組進んだ（A2）。
        val local = FakeLocal(snapshot(mineFurther()))
        val sync = syncFor(android, seen)
        val t = target(local, nas)

        // 選別画面を離れる → push（一時ファイルを書くところで止める）。
        val writing = CompletableDeferred<Unit>()
        val release = CompletableDeferred<Unit>()
        val pushFinished = CompletableDeferred<Unit>()
        val holding = java.util.concurrent.atomic.AtomicBoolean(false)
        nas.beforeWrite = { path ->
            if (path.endsWith(".tmp") && !writing.isCompleted) {
                holding.set(true)
                writing.complete(Unit)
                release.await()
                holding.set(false)
            }
        }
        // push が止まっている間に catalog.json を読んだ人がいたら、push が終わるまで返さない
        // （＝「X を読み終えたあと、後片付けをしている間に push が終わる」を決まった順に再現する）。
        nas.afterRead = { if (holding.get()) pushFinished.await() }

        val push = sync.pushIfChanged(t)
        writing.await()
        val readsBefore = nas.reads.get()
        // プロジェクト画面へ戻る → 確認。
        val check = async { sync.check(t) }
        delay(200)
        assertEquals("確認は push が終わるまで NAS を読まない", readsBefore, nas.reads.get())
        release.complete(Unit)
        assertTrue(push.await() is SyncOutcome.Pushed)
        pushFinished.complete(Unit)

        val answer = check.await()
        assertTrue("確認なし・取り込みなし: $answer", answer is SyncOutcome.Settled)
        assertEquals(mineFurther(), local.snapshot.session)
        assertTrue(sameJudgement(local, remote(nas)))
    }

    // ---- テスト 2: タイムライン A（PC の未着手の版が、Android の着手済みを消さない） ----

    @Test fun NASが未着手なら確認なしに端末の分を書きNASの版は退避する() = run {
        val nas = FakeNas()
        val a1 = written(mineAdvanced(), android, "a1")
        // 端末は A1 を書いて見た。そのあと PC が「選別を開始」を押しただけの版（P1）で上書きした。
        val pc = Me("pc-desktop-1", "DESKTOP-ABC")
        val p1 = written(untouched(), pc, null).let { flat ->
            // PC の古い形: 鍵はフォルダからの相対、photos に全部の写真を ★0 で載せる。
            flat.copy(photos = photos.associate { it.removePrefix("photo/trip/") to SidecarPhoto(0) })
        }
        val p1Bytes = put(nas, p1)
        val seen = FakeSeen().apply { save("project-1", seenAt(a1)) }
        val local = FakeLocal(snapshot(mineAdvanced()))

        val answer = syncFor(android, seen).check(target(local, nas))

        assertTrue("確認なしに書く: $answer", answer is SyncOutcome.Pushed)
        assertEquals("端末の選別状況はそのまま", mineAdvanced(), local.snapshot.session)
        assertTrue("NAS は端末の分", sameJudgement(local, remote(nas)))
        assertArrayEquals(
            "PC の版は catalog.<PC>.json に残る",
            p1Bytes, aside(nas, SidecarSync.tag(pc.id))
        )
        assertEquals(remote(nas).writeId, seen.load("project-1").seen.token)
    }

    // ---- テスト 2b: 意味が同じなら何もしない ----

    @Test fun 意味が同じなら警告も書き込みもせず控えだけ進める() = run {
        val nas = FakeNas()
        val pc = Me("pc-desktop-1", "DESKTOP-ABC")
        // 別の端末が、同じ選別状況を別の形（全部の写真を★0 で載せる・空白・時刻・版）で書いた。
        val same = written(mineAdvanced(), pc, "pc-write-9", updatedAt = 1_791_000_000_000L)
        val text = sidecarToJson(same).replace(",", ",\n   ")
        nas.files[catalog] = text.toByteArray(Charsets.UTF_8)
        val seen = FakeSeen()
        val local = FakeLocal(snapshot(mineAdvanced()))

        val answer = syncFor(android, seen).check(target(local, nas))

        assertEquals(SyncOutcome.Settled(null), answer)
        assertEquals("書かない", text, nas.text(catalog))
        assertEquals(setOf(catalog), nas.files.keys.toSet())
        assertEquals("pc-write-9", seen.load("project-1").seen.token)
    }

    // ---- テスト 2c: 端末が未着手なら確認なしに取り込む ----

    @Test fun 端末が未着手なら確認なしに取り込む() = run {
        val nas = FakeNas()
        val theirs = written(theirsAdvanced(), fold, "f1")
        put(nas, theirs)
        val seen = FakeSeen()
        val local = FakeLocal(snapshot(untouched()))

        val answer = syncFor(android, seen).check(target(local, nas))

        assertTrue("確認なしに取り込む: $answer", answer is SyncOutcome.Pulled)
        assertEquals(theirsAdvanced(), local.snapshot.session)
        assertTrue("未着手は退避しない", local.asides.isEmpty())
        assertEquals("f1", seen.load("project-1").seen.token)
        // 取り込んだあとは落ち着く（もう一度開いても何もしない）。
        assertEquals(SyncOutcome.Settled(null), syncFor(android, seen).check(target(local, nas)))
    }

    // ---- テスト 3: 書き込みの途中で切れても catalog.json は前のまま ----

    @Test fun 書き込みの途中で例外でもcatalogは前のまま() = run {
        val nas = FakeNas()
        val a1 = written(mineAdvanced(), android, "a1")
        val before = put(nas, a1)
        val seen = FakeSeen().apply { save("project-1", seenAt(a1)) }
        val local = FakeLocal(snapshot(mineFurther()))
        nas.breakWrite = { it.endsWith(".tmp") }

        val answer = syncFor(android, seen).check(target(local, nas))

        assertTrue("失敗を伝える: $answer", answer is SyncOutcome.Blocked)
        assertArrayEquals(before, nas.files[catalog])
        assertNull("ロックは放す", nas.files[SidecarSync.lockPath(folder)])
        assertEquals("控えは進めない", "a1", seen.load("project-1").seen.token)
    }

    // ---- テスト 4 と 5 択 ----

    private class Clashed(val nas: FakeNas, val local: FakeLocal, val seen: FakeSeen, val clash: SidecarClash, val theirsBytes: ByteArray)

    private suspend fun clashed(): Clashed {
        val nas = FakeNas()
        val theirsBytes = put(nas, written(theirsAdvanced(), fold, "f1"))
        val seen = FakeSeen()
        val local = FakeLocal(snapshot(mineAdvanced()))
        val answer = syncFor(android, seen).check(target(local, nas))
        assertTrue("両方着手で違う → 確認: $answer", answer is SyncOutcome.Asking)
        return Clashed(nas, local, seen, (answer as SyncOutcome.Asking).clash, theirsBytes)
    }

    @Test fun 両方着手で違えば5択の確認になり要約を1行ずつ出す() = run {
        val c = clashed()
        assertEquals(mineAdvanced(), c.local.snapshot.session)
        assertEquals("★1 以上 3 枚・ROUND 1 の途中（2 組決定・4 枚残り）", SidecarSync.describe(c.clash.mine))
        assertEquals("★1 以上 1 枚・ROUND 1 の途中（1 組決定・8 枚残り）", SidecarSync.describe(c.clash.theirsProgress))
        assertTrue(SidecarSync.canMerge(c.clash.preview))
        assertEquals("書かない", setOf(catalog), c.nas.files.keys.toSet())
    }

    @Test fun サイドカーから取り込むとNASにも端末にも退避が残る() = run {
        val c = clashed()
        val answer = syncFor(android, c.seen).resolve(target(c.local, c.nas), c.clash, ClashChoice.TakeTheirs)

        assertTrue("$answer", answer is SyncOutcome.Pulled)
        assertEquals(theirsAdvanced(), c.local.snapshot.session)
        assertEquals("端末に退避", 1, c.local.asides.size)
        val kept = aside(c.nas, SidecarSync.tag(android.id))?.let { String(it, Charsets.UTF_8) }
        assertNotNull("NAS に catalog.<自分>.json", kept)
        assertTrue(
            judgementEquivalent(
                sidecarJudgement(sidecarFromJson(kept!!)!!),
                sidecarJudgement(written(mineAdvanced(), android, "x"))
            )
        )
        assertArrayEquals("NAS の catalog.json はそのまま", c.theirsBytes, c.nas.files[catalog])
        assertEquals("f1", c.seen.load("project-1").seen.token)
    }

    @Test fun この端末の状況を残すとNASに触らず自動で書かずNASが変われば聞き直す() = run {
        val c = clashed()
        val sync = syncFor(android, c.seen)
        val t = target(c.local, c.nas)
        val answer = sync.resolve(t, c.clash, ClashChoice.KeepMine)

        assertTrue("$answer", answer is SyncOutcome.Settled)
        assertTrue(sync.detached("project-1"))
        assertArrayEquals(c.theirsBytes, c.nas.files[catalog])
        assertEquals(mineAdvanced(), c.local.snapshot.session)

        // 端末でさらに進めても、区切り・開き直しで書かない（聞き直しもしない）。
        c.local.snapshot = snapshot(mineFurther())
        val background = sync.pushIfChanged(t).await()
        assertTrue("$background", background is SyncOutcome.Settled)
        val reopened = sync.check(t)
        assertTrue("$reopened", reopened is SyncOutcome.Settled)
        assertArrayEquals("書かない", c.theirsBytes, c.nas.files[catalog])

        // NAS がまた変わった → 聞き直す。
        put(c.nas, written(theirsAdvanced().copy(survivors = listOf(p(3), p(4)), ratings = theirsAdvanced().ratings + mapOf(p(4) to 1)), fold, "f2", basedOn = "f1"))
        assertTrue(sync.check(t) is SyncOutcome.Asking)
    }

    @Test fun この端末の状況を書き込むと相手は早送りで確認なしに取り込む() = run {
        val c = clashed()
        val answer = syncFor(android, c.seen).resolve(target(c.local, c.nas), c.clash, ClashChoice.WriteMine)

        assertTrue("$answer", answer is SyncOutcome.Pushed)
        assertTrue(sameJudgement(c.local, remote(c.nas)))
        assertArrayEquals(
            "NAS の分は catalog.<相手>.json に",
            c.theirsBytes, aside(c.nas, SidecarSync.tag(fold.id))
        )
        assertEquals(remote(c.nas).writeId, c.seen.load("project-1").seen.token)

        // 相手（Fold）は f1 を見たまま何も変えていない → 確認なしに取り込む。
        val foldSeen = FakeSeen().apply { save("project-1", seenAt(written(theirsAdvanced(), fold, "f1"))) }
        val foldLocal = FakeLocal(snapshot(theirsAdvanced()))
        val theirAnswer = syncFor(fold, foldSeen).check(target(foldLocal, c.nas))
        assertTrue("$theirAnswer", theirAnswer is SyncOutcome.Pulled)
        assertEquals(mineAdvanced(), foldLocal.snapshot.session)
        assertEquals("相手の端末にも退避", 1, foldLocal.asides.size)
    }

    @Test fun 両方で残した写真のみにする() = run {
        val c = clashed()
        val answer = syncFor(android, c.seen).resolve(target(c.local, c.nas), c.clash, ClashChoice.Intersection)

        assertTrue("$answer", answer is SyncOutcome.Pushed)
        val session = c.local.snapshot.session!!
        assertTrue("混ぜた星の完了した状態", session.finished)
        // ★1・2 は相手（判定済み）で落ちた。★3 はこの端末（判定済み）で落ちた。
        // ★5 は相手ではまだ見ていない → この端末の★を採る。
        assertEquals(mapOf(p(5) to 1), session.ratings.filterValues { it > 0 })
        assertTrue(sameJudgement(c.local, remote(c.nas)))
        assertNotNull(aside(c.nas, SidecarSync.tag(fold.id)))
        assertNotNull(aside(c.nas, SidecarSync.tag(android.id)))
        assertEquals(1, c.local.asides.size)
    }

    @Test fun どちらかで残した写真をすべて残す() = run {
        val c = clashed()
        val answer = syncFor(android, c.seen).resolve(target(c.local, c.nas), c.clash, ClashChoice.Union)

        assertTrue("$answer", answer is SyncOutcome.Pushed)
        val session = c.local.snapshot.session!!
        assertTrue(session.finished)
        assertEquals(setOf(p(1), p(2), p(3), p(5)), session.ratings.filterValues { it > 0 }.keys)
        assertTrue(sameJudgement(c.local, remote(c.nas)))
        assertNotNull(aside(c.nas, SidecarSync.tag(fold.id)))
        assertNotNull(aside(c.nas, SidecarSync.tag(android.id)))
    }

    @Test fun ダイアログのあとでNASが変わっていたら実行せず聞き直す() = run {
        val c = clashed()
        put(c.nas, written(theirsAdvanced(), fold, "f2", basedOn = "f1"))
        val answer = syncFor(android, c.seen).resolve(target(c.local, c.nas), c.clash, ClashChoice.WriteMine)
        assertTrue("$answer", answer is SyncOutcome.Asking)
        assertEquals("f2", (answer as SyncOutcome.Asking).clash.token)
        assertEquals(mineAdvanced(), c.local.snapshot.session)
    }

    // ---- 早送り ----

    @Test fun 早送りは確認なしに取り込み端末の分を退避する() = run {
        val nas = FakeNas()
        val f1 = written(theirsAdvanced(), fold, "f1")
        val seen = FakeSeen().apply { save("project-1", seenAt(f1)) }
        val local = FakeLocal(snapshot(theirsAdvanced()))
        // Fold が f1 から続けて書いた（f2）。この端末は f1 のあと何もしていない。
        val further = theirsAdvanced().copy(
            history = theirsAdvanced().history + Decision(photos.subList(4, 8), listOf(p(6)), null, emptyMap()),
            ratings = theirsAdvanced().ratings + mapOf(p(6) to 1),
            current = photos.drop(8), queue = emptyList()
        )
        put(nas, written(further, fold, "f2", basedOn = "f1"))

        val answer = syncFor(android, seen).check(target(local, nas))

        assertTrue("$answer", answer is SyncOutcome.Pulled)
        assertEquals(further, local.snapshot.session)
        assertEquals(1, local.asides.size)
        assertNotNull(aside(nas, SidecarSync.tag(android.id)))
    }

    // ---- そのほか ----

    @Test fun 背面への移動では取り込みも確認もしない() = run {
        val c = clashed()
        val answer = syncFor(android, c.seen).pushIfChanged(target(c.local, c.nas)).await()
        assertEquals(SyncOutcome.Deferred, answer)
        assertEquals(setOf(catalog), c.nas.files.keys.toSet())
    }

    @Test fun ほかの端末のロックが新しければ書かない() = run {
        val nas = FakeNas()
        val local = FakeLocal(snapshot(mineAdvanced()))
        val sync = syncFor(android, FakeSeen())
        nas.files[SidecarSync.lockPath(folder)] = "{\"device\":\"pc\",\"at\":${clock}}".toByteArray()
        assertTrue(sync.check(target(local, nas)) is SyncOutcome.Blocked)
        assertNull(nas.files[catalog])
        // 古いロック（60 秒より前）は捨てて書く。
        nas.files[SidecarSync.lockPath(folder)] = "{\"device\":\"pc\",\"at\":${clock - 120_000}}".toByteArray()
        assertTrue(sync.check(target(local, nas)) is SyncOutcome.Pushed)
        assertNull(nas.files[SidecarSync.lockPath(folder)])
        assertNotNull(nas.files[catalog])
    }

    @Test fun 古いAndroidの形のcatalogを読める() = run {
        val nas = FakeNas()
        // version 無し・鍵は共有の根から（フォルダ名付き）・手直しは l/r/d。端末の 1 つ前の版の Android が書いたもの。
        val old = """
            {"updatedAt": 1700000000000, "updatedBy": "old-android", "updatedByName": "SM-F971C",
             "photos": {"photo/trip/IMG_001.JPG": {"rating": 1}},
             "burstOverrides": [{"l": "photo/trip/IMG_001.JPG", "r": "photo/trip/IMG_002.JPG", "d": "split"}],
             "sessions": {"tournament": ${uniffi.photo_curator_core.sessionToJson(theirsAdvanced())}}}
        """.trimIndent()
        nas.files[catalog] = old.toByteArray()
        val local = FakeLocal(snapshot(untouched()), keys = photos)
        val seen = FakeSeen()

        val answer = syncFor(android, seen).check(target(local, nas))

        assertTrue("$answer", answer is SyncOutcome.Pulled)
        assertEquals("端末の鍵（フォルダ名付き）に戻る", theirsAdvanced(), local.snapshot.session)
        assertEquals(listOf(p(1) to p(2)), local.snapshot.overrides.map { it.left to it.right })
        assertEquals("legacy:1700000000000:old-android", seen.load("project-1").seen.token)
    }

    @Test fun 写真の場所が違う記録は取り込まない() = run {
        val nas = FakeNas()
        val elsewhere = theirsAdvanced().let { s ->
            val move = { k: String -> k.replace("IMG_", "DSC_") }
            s.copy(
                queue = s.queue.map(move), current = s.current.map(move), survivors = s.survivors.map(move),
                ratings = s.ratings.mapKeys { move(it.key) },
                history = s.history.map { d -> d.copy(group = d.group.map(move), chosen = d.chosen.map(move)) }
            )
        }
        put(nas, written(elsewhere, fold, "f1"))
        val local = FakeLocal(snapshot(untouched()), keys = photos)
        val answer = syncFor(android, FakeSeen()).check(target(local, nas))
        assertTrue("$answer", answer is SyncOutcome.Blocked)
        assertEquals(untouched(), local.snapshot.session)
    }

    @Test fun まだ無くて端末が未着手なら書かない() = run {
        val nas = FakeNas()
        val local = FakeLocal(snapshot(untouched()))
        assertEquals(SyncOutcome.Settled(null), syncFor(android, FakeSeen()).check(target(local, nas)))
        assertTrue(nas.files.isEmpty())
    }

    @Test fun 古い控えから移すとdirtyでなければ変更なしと見なす() = run {
        val nas = FakeNas()
        val a1 = written(mineAdvanced(), android, null, updatedAt = 1_700_000_000_000L)
        put(nas, a1)
        // seenAt/seenBy から作った legacy の token。dirty=false だったので比較キーは端末から埋める。
        val seen = FakeSeen().apply {
            save("project-1", SeenState(SeenRecord("legacy:1700000000000:${android.id}", "", null), false, keyFromLocal = true))
        }
        val local = FakeLocal(snapshot(mineAdvanced()))
        val answer = syncFor(android, seen).check(target(local, nas))
        assertTrue("$answer", answer is SyncOutcome.Settled)
        assertFalse(seen.load("project-1").keyFromLocal)
    }

    // ---- U44 D2: 端末の選別状況を読めなかったとき ----

    @Test fun 端末の記録を読めなければ判断も書き込みもせず理由を出す() = run {
        val nas = FakeNas()
        val a1 = written(mineAdvanced(), android, "a1")
        val before = put(nas, a1)
        val seen = FakeSeen().apply { save("project-1", seenAt(a1)) }
        val local = FakeLocal(snapshot(mineAdvanced())).apply { broken = "session-project-1.json の形が違う" }

        val answer = syncFor(android, seen).check(target(local, nas))

        assertTrue("$answer", answer is SyncOutcome.Blocked)
        assertTrue((answer as SyncOutcome.Blocked).reason, answer.reason.contains("この端末の選別の記録を読めませんでした"))
        assertArrayEquals("NAS はそのまま", before, nas.files[catalog])
        assertEquals("NAS に何も増やさない", setOf(catalog), nas.files.keys.toSet())
        assertTrue("読めなかった印が残る", seen.load("project-1").localBroken)
        assertEquals("a1", seen.load("project-1").seen.token)
        assertTrue(local.asides.isEmpty())
    }

    @Test fun 読めなかったあとは見た版のままでも端末の分で自動に上書きせず確認する() = run {
        val nas = FakeNas()
        val a1 = written(mineAdvanced(), android, "a1")
        val before = put(nas, a1)
        val seen = FakeSeen().apply { save("project-1", seenAt(a1)) }
        val local = FakeLocal(snapshot(mineAdvanced())).apply { broken = "形が違う" }
        val sync = syncFor(android, seen)
        val t = target(local, nas)
        assertTrue(sync.check(t) is SyncOutcome.Blocked)

        // アプリの更新で Session を読めなくなり、端末には学習した境目だけが残った（Session は作り直し前）。
        local.broken = null
        local.snapshot = LocalSnapshot(null, emptyList(), 9, null)
        val background = sync.pushIfChanged(t).await()
        assertEquals("背面では書かない", SyncOutcome.Deferred, background)
        val answer = sync.check(t)

        assertTrue("見た版のままでも上書きせず確認: $answer", answer is SyncOutcome.Asking)
        assertArrayEquals("NAS の Session は残る", before, nas.files[catalog])
    }

    @Test fun 読めなかったあと端末が空ならNASから取り込み印を下ろす() = run {
        val nas = FakeNas()
        val a1 = written(mineAdvanced(), android, "a1")
        put(nas, a1)
        val seen = FakeSeen().apply { save("project-1", seenAt(a1).copy(localBroken = true)) }
        val local = FakeLocal(snapshot(null))

        val answer = syncFor(android, seen).check(target(local, nas))

        assertTrue("$answer", answer is SyncOutcome.Pulled)
        assertEquals(mineAdvanced(), local.snapshot.session)
        assertFalse(seen.load("project-1").localBroken)
    }

    // ---- U44 D4: 退避を上書きしない・最新の 5 つを残す・退避できてから変える ----

    @Test fun NASの退避は書くたびに別の名前で残り最新の5つまで() = run {
        val nas = FakeNas()
        val pc = Me("pc-desktop-1", "DESKTOP-ABC")
        val seen = FakeSeen()
        val local = FakeLocal(snapshot(mineAdvanced()))
        val sync = syncFor(android, seen)
        val versions = ArrayList<ByteArray>()
        // PC が「選別を開始」を押しただけの版で、7 回上書きした（そのたびに Android が退避して書く）。
        repeat(7) { i ->
            clock += 5_000
            val bytes = put(nas, written(untouched(), pc, "pc-$i", updatedAt = 1_789_000_000_000L + i))
            versions += bytes
            val answer = sync.check(target(local, nas))
            assertTrue("$i: $answer", answer is SyncOutcome.Pushed)
        }

        val kept = asides(nas, SidecarSync.tag(pc.id))
        assertEquals("最新の 5 つ", 5, kept.size)
        assertEquals(
            "残るのは新しい 5 つの中身",
            versions.takeLast(5).reversed().map { String(it, Charsets.UTF_8) },
            kept.map { nas.text(it) }
        )
    }

    @Test fun 時刻の無い古い退避と別の端末の退避は片付けない() {
        val names = listOf(
            "catalog.pc-desktop-1.json",
            "catalog.fold-0000002.20261001000000.json",
            "catalog.pc-desktop-1.20261001000001.json",
            "catalog.pc-desktop-1.20261001000002.json",
            "catalog.pc-desktop-1.20261001000002-2.json",
            "catalog.json"
        )
        assertEquals(
            listOf("catalog.pc-desktop-1.20261001000002.json", "catalog.pc-desktop-1.20261001000001.json"),
            SidecarSync.asidesToDrop(names, "pc-desktop-1", 1)
        )
    }

    @Test fun 取り込む前にNASへ退避できなければ取り込まない() = run {
        val c = clashed()
        c.nas.refuse = { SidecarSync.isAsideOf(it.substringAfterLast('\\'), SidecarSync.tag(android.id)) }

        val answer = syncFor(android, c.seen).resolve(target(c.local, c.nas), c.clash, ClashChoice.TakeTheirs)

        assertTrue("$answer", answer is SyncOutcome.Blocked)
        assertEquals("端末はそのまま", mineAdvanced(), c.local.snapshot.session)
        assertArrayEquals(c.theirsBytes, c.nas.files[catalog])
        assertEquals("控えは進めない", "", c.seen.load("project-1").seen.token)
    }

    @Test fun 書く前に相手の版を退避できなければ書かない() = run {
        val c = clashed()
        c.nas.refuse = { SidecarSync.isAsideOf(it.substringAfterLast('\\'), SidecarSync.tag(fold.id)) }

        val answer = syncFor(android, c.seen).resolve(target(c.local, c.nas), c.clash, ClashChoice.WriteMine)

        assertTrue("$answer", answer is SyncOutcome.Blocked)
        assertArrayEquals("NAS はそのまま", c.theirsBytes, c.nas.files[catalog])
        assertNull("ロックは放す", c.nas.files[SidecarSync.lockPath(folder)])
    }

    // ---- U44 D5: ロックを壊しすぎない・他人のロックを消さない ----

    private val lock = SidecarSync.lockPath(folder)

    @Test fun 中身が空のロックは更新時刻が新しければ壊さない() = run {
        val nas = FakeNas().apply { nasNow = clock }
        nas.files[lock] = ByteArray(0)
        nas.mtimes[lock] = clock - 1_000
        val local = FakeLocal(snapshot(mineAdvanced()))

        val answer = syncFor(android, FakeSeen()).check(target(local, nas))

        assertTrue("書かない: $answer", answer is SyncOutcome.Blocked)
        assertNotNull("ロックはそのまま", nas.files[lock])
        assertNull(nas.files[catalog])
    }

    @Test fun 中身が空のロックでも更新時刻が古ければ壊して書く() = run {
        val nas = FakeNas().apply { nasNow = clock }
        nas.files[lock] = ByteArray(0)
        nas.mtimes[lock] = clock - 120_000
        val local = FakeLocal(snapshot(mineAdvanced()))

        val answer = syncFor(android, FakeSeen()).check(target(local, nas))

        assertTrue("$answer", answer is SyncOutcome.Pushed)
        assertNull(nas.files[lock])
    }

    @Test fun 中のatが古くても更新時刻が新しければ壊さない() = run {
        val nas = FakeNas().apply { nasNow = clock }
        nas.files[lock] = "{\"holder\":\"pc\",\"at\":${clock - 600_000}}".toByteArray()
        nas.mtimes[lock] = clock - 1_000
        val local = FakeLocal(snapshot(mineAdvanced()))

        val answer = syncFor(android, FakeSeen()).check(target(local, nas))

        assertTrue("時計がずれていても生きているロックは壊さない: $answer", answer is SyncOutcome.Blocked)
        assertNotNull(nas.files[lock])
    }

    @Test fun 自分のロックが置き換わっていたら放すときに消さない() = run {
        val nas = FakeNas()
        val local = FakeLocal(snapshot(mineAdvanced()))
        val theirLock = "{\"holder\":\"pc\",\"at\":${clock}}".toByteArray()
        // 書いている途中で、ほかの端末がこの端末のロックを古いと見なして取り直した。
        nas.beforeWrite = { path -> if (path.endsWith(".tmp")) nas.files[lock] = theirLock }

        syncFor(android, FakeSeen()).check(target(local, nas))

        assertArrayEquals("ほかの端末のロックは残す", theirLock, nas.files[lock])
    }

    @Test fun 古いロックを壊す前に読み直して変わっていれば壊さない() = run {
        val nas = FakeNas()
        nas.files[lock] = "{\"holder\":\"pc\",\"at\":${clock - 120_000}}".toByteArray()
        val fresh = "{\"holder\":\"fold\",\"at\":${clock}}".toByteArray()
        val lockReads = AtomicInteger()
        // 古いロックを読んだ直後に、ほかの端末がそれを壊して自分のロックを取った。
        nas.afterAnyRead = { path ->
            if (path == lock && lockReads.incrementAndGet() == 1) {
                nas.files[lock] = fresh
                nas.mtimes[lock] = clock
            }
        }
        nas.nasNow = clock
        val local = FakeLocal(snapshot(mineAdvanced()))

        val answer = syncFor(android, FakeSeen()).check(target(local, nas))

        assertTrue("$answer", answer is SyncOutcome.Blocked)
        assertArrayEquals("取り直したロックは消さない", fresh, nas.files[lock])
        assertNull(nas.files[catalog])
    }

    @Test fun ロックの形はPCと同じholderとat() = run {
        val nas = FakeNas()
        var seenLock: String? = null
        nas.beforeWrite = { path -> if (path.endsWith(".tmp")) seenLock = nas.text(lock) }
        syncFor(android, FakeSeen()).check(target(FakeLocal(snapshot(mineAdvanced())), nas))
        assertTrue("$seenLock", seenLock!!.contains("\"holder\":\"${android.id}\"") && Regex("\"at\":\\d+").containsMatchIn(seenLock!!))
    }

    // ---- U44 D7: 混ぜた版は NAS に書けてから端末に入れる ----

    @Test fun 混ぜた版をNASに書けなければ端末は元のまま() = run {
        val c = clashed()
        // ほかの端末が書いている（新しいロック）。
        c.nas.files[lock] = "{\"holder\":\"pc\",\"at\":${clock}}".toByteArray()
        c.nas.mtimes[lock] = clock
        c.nas.nasNow = clock

        val answer = syncFor(android, c.seen).resolve(target(c.local, c.nas), c.clash, ClashChoice.Intersection)

        assertTrue("$answer", answer is SyncOutcome.Blocked)
        assertEquals("端末は混ぜないまま", mineAdvanced(), c.local.snapshot.session)
        assertArrayEquals("NAS もそのまま", c.theirsBytes, c.nas.files[catalog])
        assertEquals("控えは進めない", "", c.seen.load("project-1").seen.token)
        assertEquals("退避は済んでいる", 1, c.local.asides.size)
    }

    @Test fun 混ぜた版を書いている途中で切れても端末は元のまま() = run {
        val c = clashed()
        c.nas.breakWrite = { it.endsWith(".tmp") }

        val answer = syncFor(android, c.seen).resolve(target(c.local, c.nas), c.clash, ClashChoice.Union)

        assertTrue("$answer", answer is SyncOutcome.Blocked)
        assertEquals(mineAdvanced(), c.local.snapshot.session)
        assertArrayEquals(c.theirsBytes, c.nas.files[catalog])
    }

    @Test fun 混ぜた版を書けたら端末に入れて控えを書いた版にする() = run {
        val c = clashed()
        val answer = syncFor(android, c.seen).resolve(target(c.local, c.nas), c.clash, ClashChoice.Union)

        assertTrue("$answer", answer is SyncOutcome.Pushed)
        val state = c.seen.load("project-1")
        assertEquals(remote(c.nas).writeId, state.seen.token)
        assertEquals("控えの比較キーは端末に入れたあとの値", judgementKey(sidecarJudgement(localSidecar(c.local))), state.seen.key)
        // もう一度開いても何もしない（混ぜた版で落ち着く）。
        assertEquals(SyncOutcome.Settled(null), syncFor(android, c.seen).check(target(c.local, c.nas)))
    }

    // ---- U44 D11: 書きかけの残りかすを、開いたときに古いものだけ片付ける ----

    private fun leftoverNas(): Pair<FakeNas, Sidecar> {
        val nas = FakeNas()
        val a1 = written(mineAdvanced(), android, "a1")
        put(nas, a1)
        val head = SidecarSync.sidecarDir(folder) + "\\"
        val old = clock - 2 * 60 * 60 * 1000L
        for (name in listOf(".catalog.w9old.tmp", ".catalog.json.0f3a.tmp", "catalog.json.1a2b3c4d.writing", "catalog.x.tmp")) {
            nas.files[head + name] = "half".toByteArray()
            nas.mtimes[head + name] = old
        }
        nas.files[head + ".catalog.w9new.tmp"] = "writing now".toByteArray()
        nas.mtimes[head + ".catalog.w9new.tmp"] = clock - 10_000
        nas.files[head + "catalog.pc-desktop-1.20261001000000.json"] = "aside".toByteArray()
        nas.mtimes[head + "catalog.pc-desktop-1.20261001000000.json"] = old
        nas.mtimes[catalog] = old
        nas.nasNow = clock
        return nas to a1
    }

    @Test fun 開いたときに古い書きかけだけ片付ける() = run {
        val (nas, a1) = leftoverNas()
        val head = SidecarSync.sidecarDir(folder) + "\\"
        val seen = FakeSeen().apply { save("project-1", seenAt(a1)) }

        val answer = syncFor(android, seen).check(target(FakeLocal(snapshot(mineAdvanced())), nas))

        assertTrue("$answer", answer is SyncOutcome.Settled)
        assertEquals(
            "残るのは catalog.json・退避・進行中の一時ファイル",
            setOf(catalog, head + ".catalog.w9new.tmp", head + "catalog.pc-desktop-1.20261001000000.json"),
            nas.files.keys.toSet()
        )
    }

    @Test fun ほかの端末が書いている間と背面では片付けない() = run {
        val (nas, a1) = leftoverNas()
        val seen = FakeSeen().apply { save("project-1", seenAt(a1)) }
        val before = nas.files.keys.toSet()
        val local = FakeLocal(snapshot(mineAdvanced()))
        val sync = syncFor(android, seen)

        assertTrue(sync.pushIfChanged(target(local, nas)).await() is SyncOutcome.Settled)
        assertEquals("背面では片付けない", before, nas.files.keys.toSet())

        nas.files[lock] = "{\"holder\":\"pc\",\"at\":${clock}}".toByteArray()
        nas.mtimes[lock] = clock
        sync.check(target(local, nas))
        assertEquals("ロックがあれば片付けない", before + lock, nas.files.keys.toSet())
    }

    @Test fun 片付ける名前は書きかけだけ() {
        val now = 10_000_000_000L
        val old = now - 2 * 60 * 60 * 1000L
        val entries = listOf(
            CatalogEntry(".catalog.abc.tmp", old),
            CatalogEntry(".catalog.json.0f3a.tmp", old),
            CatalogEntry("catalog.json.1a2b3c4d.writing", old),
            CatalogEntry("catalog.json", old),
            CatalogEntry("catalog.lock", old),
            CatalogEntry("catalog.pc.20261001000000.json", old),
            CatalogEntry("catalog.json.broken-20261001000000.json", old),
            CatalogEntry(".catalog.fresh.tmp", now - 60_000)
        )
        assertEquals(
            listOf(".catalog.abc.tmp", ".catalog.json.0f3a.tmp", "catalog.json.1a2b3c4d.writing"),
            SidecarSync.leftoversToClean(entries, now)
        )
    }

    @Test fun 文言はPCと同じ形() {
        assertEquals("NAS の記録と、この端末の記録が違います", SidecarSync.CLASH_TITLE)
        assertEquals("この端末（Pixel 8）", SidecarSync.mineLabel("Pixel 8"))
    }
}
