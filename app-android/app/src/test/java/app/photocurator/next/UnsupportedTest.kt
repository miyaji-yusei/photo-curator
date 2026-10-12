package app.photocurator.next

import app.photocurator.next.Analyse.HashResult
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * U58（Android）: 解析できなかった写真の扱い。
 * 非対応は原本が変わるまで試さず、分母と選別の対象から外す。一時的な失敗は試し直す。
 */
class UnsupportedTest {
    private fun photo(rel: String, size: Long = 10) =
        Photo(id = 1, name = rel.substringAfterLast('/'), relativePath = rel, size = size, takenAt = 1)

    private fun made(size: Long = 10) = HashResult.Made(Fingerprint(Analyse.VERSION, size, "ab"))

    private fun bad(size: Long = 10, kind: FailureKind = FailureKind.Unsupported) =
        Failure(kind, "x", size, Analyse.VERSION, 0)

    /** 準備 1 回ぶん（端末の写真を 1 枚ずつ読む道筋）。読まれた順を残す。 */
    private class Run(
        val photos: List<Photo>,
        val cached: Map<String, Fingerprint> = emptyMap(),
        val known: Map<String, Failure> = emptyMap(),
        val read: (Photo) -> HashResult
    ) {
        val readPaths = ArrayList<String>()
        val progress = ArrayList<Pair<Int, Int>>()
        lateinit var result: Analyse.Prints

        fun go(): Run {
            val sweep = Analyse.Sweep(photos, cached, known, { d, t -> progress += d to t }, {})
            runBlocking {
                Analyse.sweepLocal(sweep) { p -> readPaths += p.relativePath; read(p) }
            }
            result = sweep.finish()
            return this
        }
    }

    // ---- 再試行するかしないか ----

    @Test fun 非対応は二回目に読まれない() {
        val photos = listOf(photo("ok.jpg"), photo("bad.jpg"))
        val first = Run(photos) { if (it.relativePath == "bad.jpg") HashResult.Unsupported("壊れている") else made() }.go()
        assertEquals(listOf("ok.jpg", "bad.jpg"), first.readPaths)
        assertEquals(FailureKind.Unsupported, first.result.failures["bad.jpg"]?.kind)
        assertNull(first.result.prints["bad.jpg"])

        val second = Run(photos, first.result.prints, first.result.failures) {
            error("読んではいけない: " + it.relativePath)
        }.go()
        assertTrue(second.readPaths.isEmpty())
        // 控えは残る（一覧に出し続ける）。
        assertEquals(FailureKind.Unsupported, second.result.failures["bad.jpg"]?.kind)
    }

    @Test fun 原本が変わったら非対応も試し直す() {
        val first = Run(listOf(photo("a.jpg", 10))) { HashResult.Unsupported("壊れている") }.go()
        val replaced = Run(listOf(photo("a.jpg", 99)), first.result.prints, first.result.failures) { made(99) }.go()
        assertEquals(listOf("a.jpg"), replaced.readPaths)
        assertEquals(99L, replaced.result.prints["a.jpg"]?.size)
        // 直ったので控えから消える。
        assertNull(replaced.result.failures["a.jpg"])
    }

    @Test fun 作り方の版が変わったら非対応も試し直す() {
        val old = Failure(FailureKind.Unsupported, "x", 10, Analyse.VERSION - 1, 0)
        val run = Run(listOf(photo("a.jpg")), known = mapOf("a.jpg" to old)) { made() }.go()
        assertEquals(listOf("a.jpg"), run.readPaths)
    }

    @Test fun 一時的な失敗は次も試し直す() {
        val photos = listOf(photo("a.jpg"))
        val first = Run(photos) { HashResult.Transient("NAS から読めませんでした") }.go()
        assertEquals(FailureKind.Transient, first.result.failures["a.jpg"]?.kind)
        assertNull(first.result.prints["a.jpg"])

        val second = Run(photos, first.result.prints, first.result.failures) { made() }.go()
        assertEquals(listOf("a.jpg"), second.readPaths)
        assertNull(second.result.failures["a.jpg"])
        assertEquals("ab", second.result.prints["a.jpg"]?.hash)
    }

    @Test fun 非対応から一時的な失敗に変わっても次は試し直される() {
        val first = Run(listOf(photo("a.jpg", 10))) { HashResult.Unsupported("壊れている") }.go()
        val replaced = Run(listOf(photo("a.jpg", 20)), first.result.prints, first.result.failures) {
            HashResult.Transient("開けない")
        }.go()
        assertEquals(FailureKind.Transient, replaced.result.failures["a.jpg"]?.kind)
        val again = Run(listOf(photo("a.jpg", 20)), replaced.result.prints, replaced.result.failures) { made(20) }.go()
        assertEquals(listOf("a.jpg"), again.readPaths)
    }

    @Test fun 読めた分は読み直さない() {
        val photos = listOf(photo("a.jpg"), photo("b.jpg"))
        val first = Run(photos) { made() }.go()
        val second = Run(photos, first.result.prints) { error("読んではいけない") }.go()
        assertTrue(second.readPaths.isEmpty())
    }

    // ---- 分母・選別の対象 ----

    @Test fun 非対応は進みの分母に入れない() {
        val known = mapOf("bad.jpg" to bad())
        val photos = listOf(photo("a.jpg"), photo("bad.jpg"), photo("c.jpg"))
        val run = Run(photos, known = known) { made() }.go()
        // 最初から非対応と分かっている 1 枚は数えない: 2 枚中 2 枚。
        assertEquals(2 to 2, run.progress.last())
        assertEquals(listOf("a.jpg", "c.jpg"), run.readPaths)
    }

    @Test fun 今回わかった非対応も分母から外れる() {
        val photos = (1..4).map { photo("p$it.jpg") }
        val run = Run(photos) { if (it.relativePath == "p2.jpg") HashResult.Unsupported("壊れている") else made() }.go()
        assertEquals(3 to 3, run.progress.last())
    }

    @Test fun 全部が非対応なら分母は0() {
        val run = Run(listOf(photo("a.jpg"))) { HashResult.Unsupported("壊れている") }.go()
        assertEquals(0 to 0, run.progress.last())
    }

    @Test fun 準備済みの判定は非対応を片付いたものとして数える() {
        val photos = listOf(photo("a.jpg"), photo("bad.jpg"))
        val prints = mapOf("a.jpg" to Fingerprint(Analyse.VERSION, 10, "ab"))
        assertFalse(Analyse.allUpToDate(photos, prints))
        assertTrue(Analyse.allUpToDate(photos, prints, mapOf("bad.jpg" to bad())))
        // 一時的な失敗は片付いていない。
        assertFalse(Analyse.allUpToDate(photos, prints, mapOf("bad.jpg" to bad(kind = FailureKind.Transient))))
        // 原本が変わっていたら片付いていない。
        assertFalse(
            Analyse.allUpToDate(listOf(photo("a.jpg"), photo("bad.jpg", 11)), prints, mapOf("bad.jpg" to bad()))
        )
    }

    @Test fun 非対応だけなら網へつながない() {
        assertFalse(Prepare.needsNetwork(listOf(photo("bad.jpg")), emptyMap(), mapOf("bad.jpg" to bad())))
        assertTrue(Prepare.needsNetwork(listOf(photo("bad.jpg")), emptyMap()))
    }

    @Test fun 選別に渡す顔ぶれだけ非対応を外し_元の一覧は変えない() {
        val photos = listOf(photo("a.jpg"), photo("bad.jpg"), photo("c.jpg"))
        val failures = mapOf("bad.jpg" to bad(), "c.jpg" to bad(kind = FailureKind.Transient))
        assertEquals(listOf("a.jpg", "c.jpg"), Prepare.selectable(photos, failures).map { it.relativePath })
        // 控えが無ければそのまま。
        assertEquals(photos, Prepare.selectable(photos, emptyMap()))
        // 原本が変わった非対応は対象に戻る。
        assertEquals(3, Prepare.selectable(photos.map { it.copy(size = 11) }, failures).size)
    }

    @Test fun 対象の枚数は非対応を引く() {
        val photos = listOf(photo("a.jpg"), photo("bad.jpg"))
        assertEquals(1, Failures.workableCount(photos, mapOf("bad.jpg" to bad())))
        assertEquals(1, Failures.unsupportedCount(photos, mapOf("bad.jpg" to bad())))
        assertEquals(2, Failures.workableCount(photos, emptyMap()))
    }

    @Test fun 顔ぶれから消えた写真の控えは刈り込まれる() {
        val stale = mapOf("gone.jpg" to bad())
        val run = Run(listOf(photo("a.jpg")), known = stale) { made() }.go()
        assertTrue(run.result.failures.isEmpty())
    }

    // ---- 一覧 ----

    @Test fun 一覧は名前と理由と種類を相対パス順で返す() {
        val photos = listOf(photo("z/b.jpg"), photo("a/a.cr2"), photo("m.jpg"), photo("ok.jpg"))
        val failures = mapOf(
            "z/b.jpg" to Failure(FailureKind.Transient, "NAS から読めませんでした", 10, Analyse.VERSION, 1),
            "a/a.cr2" to Failure(FailureKind.Unsupported, "RAW に埋め込みのプレビューがありません", 10, Analyse.VERSION, 1),
            // 原本が変わった非対応は試し直す対象なので出さない。
            "m.jpg" to Failure(FailureKind.Unsupported, "古い", 999, Analyse.VERSION, 1),
            // 顔ぶれに無い。
            "gone.jpg" to bad()
        )
        val rows = Failures.rows(photos, failures)
        assertEquals(listOf("a/a.cr2", "z/b.jpg"), rows.map { it.relativePath })
        assertEquals(listOf("a.cr2", "b.jpg"), rows.map { it.name })
        assertEquals(listOf(FailureKind.Unsupported, FailureKind.Transient), rows.map { it.kind })
        assertEquals("RAW に埋め込みのプレビューがありません", rows[0].reason)
    }

    // ---- 控えの読み書き ----

    @Test fun 控えは書いて読むと元に戻る() {
        val failures = mapOf(
            "日本語/a.jpg" to Failure(FailureKind.Unsupported, "壊れている", 123, Analyse.VERSION, 5),
            "b.jpg" to Failure(FailureKind.Transient, "開けない", 7, Analyse.VERSION, 6)
        )
        assertEquals(failures, Failures.parse(Failures.serialize(failures)))
    }

    @Test fun 壊れた控えでも落ちず空になる() {
        assertTrue(Failures.parse("").isEmpty())
        assertTrue(Failures.parse("{ 壊れた").isEmpty())
        assertTrue(Failures.parse("[1,2,3]").isEmpty())
        assertTrue(Failures.parse("null").isEmpty())
    }

    @Test fun 一件だけ壊れていれば残りは読む() {
        val text = """{
          "ok.jpg": {"k":"unsupported","r":"壊れている","size":10,"v":${Analyse.VERSION},"at":1},
          "noSize.jpg": {"k":"unsupported","r":"x"},
          "future.jpg": {"k":"quarantined","r":"x","size":1,"v":1},
          "str.jpg": "文字列"
        }"""
        assertEquals(setOf("ok.jpg"), Failures.parse(text).keys)
    }

    @Test fun 壊れた控えは非対応を忘れるだけで写真は選別に残る() {
        // 控えが読めない＝空＝全部試し直す（安全側）。
        val photos = listOf(photo("a.jpg"))
        assertEquals(photos, Prepare.selectable(photos, Failures.parse("{ 壊れた")))
    }

    @Test fun 控えのファイル名は似たフォルダでも混ざらない() {
        // B2: 英数字以外を _ に潰すだけだと、同じ長さの日本語のフォルダが同じ名前になる。
        val a = Failures.fileName("nas1|写真/旅行")
        val b = Failures.fileName("nas1|写真/家族")
        assertNotEquals(a, b)
        assertEquals(a, Failures.fileName("nas1|写真/旅行"))
        assertTrue(a.startsWith("failures-") && a.endsWith(".json"))
    }
}
