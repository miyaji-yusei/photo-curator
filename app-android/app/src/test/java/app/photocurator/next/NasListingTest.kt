package app.photocurator.next

import app.photocurator.next.Prepare.ListingDecision
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import kotlinx.coroutines.runBlocking
import org.junit.Test

/**
 * U50（レビュー D8）: NAS の一覧が途中のフォルダの失敗・上限で欠けたとき、**欠けた一覧で
 * 控え（Listing）とハッシュ値を置き換えない**。偽の `share.list`（[Smb.scanDeep] の差し込み口）で確かめる。
 */
class NasListingTest {
    private fun file(name: String) = Smb.Entry(name, folder = false, size = 10, modifiedAt = 1)
    private fun dir(name: String) = Smb.Entry(name, folder = true, size = 0, modifiedAt = 1)

    /** 偽の NAS。道筋 → 中身。`broken` の道筋は読むと例外（時間切れなど）。 */
    private class FakeShare(
        private val tree: Map<String, List<Smb.Entry>>,
        private val broken: Set<String> = emptySet()
    ) {
        val listed = mutableListOf<String>()
        fun list(path: String): List<Smb.Entry> {
            listed += path
            if (path in broken) throw java.io.IOException("timed out: $path")
            return tree[path] ?: emptyList()
        }
    }

    private val tree = mapOf(
        "shoot" to listOf(file("a.jpg"), dir("raw"), dir("jpg"), dir(".photo-curator")),
        "shoot\\raw" to listOf(file("b.cr2")),
        "shoot\\jpg" to listOf(file("c.jpg"), file("notes.txt"))
    )

    @Test fun 全部読めれば完全な一覧になる() {
        val scan = Smb.scanDeep("shoot", FakeShare(tree)::list)
        assertTrue(scan.complete)
        // 同時刻は道筋の順（今までと同じ並べ方）。
        assertEquals(listOf("shoot\\a.jpg", "shoot\\jpg\\c.jpg", "shoot\\raw\\b.cr2"), scan.photos.map { it.path })
    }

    @Test fun 途中のフォルダが読めなければ不完全として返す() {
        val scan = Smb.scanDeep("shoot", FakeShare(tree, broken = setOf("shoot\\raw"))::list)
        assertFalse("読めなかったフォルダがあるのに完全扱いにしない", scan.complete)
        assertEquals(listOf("shoot\\raw"), scan.unreadable)
        // 読めた分は返す（理由と一緒に）。使うかどうかは呼ぶ側が決める。
        assertEquals(listOf("shoot\\a.jpg", "shoot\\jpg\\c.jpg"), scan.photos.map { it.path })
    }

    @Test fun 選んだフォルダそのものが読めなければ失敗にする() {
        try {
            Smb.scanDeep("shoot", FakeShare(tree, broken = setOf("shoot"))::list)
            fail("根が読めないのを空の一覧にしない")
        } catch (expected: java.io.IOException) {
        }
    }

    @Test fun 深さの上限を超えるフォルダがあれば黙って切らずに不完全にする() {
        val deep = mapOf(
            "r" to listOf(file("1.jpg"), dir("a")),
            "r\\a" to listOf(file("2.jpg"), dir("b")),
            "r\\a\\b" to listOf(file("3.jpg"))
        )
        val cut = Smb.scanDeep("r", FakeShare(deep)::list, maxDepth = 1)
        assertFalse(cut.complete)
        assertTrue(cut.truncated)
        val whole = Smb.scanDeep("r", FakeShare(deep)::list, maxDepth = 2)
        assertTrue(whole.complete)
        assertEquals(3, whole.photos.size)
    }

    @Test fun 枚数の上限に達したら黙って切らずに不完全にする() {
        val three = mapOf("r" to listOf(file("1.jpg"), file("2.jpg"), file("3.jpg")))
        val cut = Smb.scanDeep("r", FakeShare(three)::list, limit = 2)
        assertFalse(cut.complete)
        assertTrue(cut.truncated)
        // ちょうど上限なら欠けていない。
        assertTrue(Smb.scanDeep("r", FakeShare(three)::list, limit = 3).complete)
    }

    @Test fun 上限は前より小さくしない() {
        assertTrue(Smb.DEEP_MAX_DEPTH >= 4)
        assertTrue(Smb.DEEP_LIMIT >= 20000)
    }

    private fun photo(rel: String) = Photo(id = 1, name = rel, relativePath = rel, size = 1, takenAt = 1)

    @Test fun 不完全な一覧では前の控えを置き換えない() {
        val previous = listOf(photo("a"), photo("b"))
        val fresh = listOf(photo("a"))
        assertEquals(ListingDecision.Incomplete, Prepare.decideListing(previous, fresh, complete = false))
        // 前が無くても、欠けた一覧を控えにしない（次からそれが「前の一覧」になってしまう）。
        assertEquals(ListingDecision.Incomplete, Prepare.decideListing(null, fresh, complete = false))
        assertEquals(ListingDecision.Incomplete, Prepare.decideListing(emptyList(), emptyList(), complete = false))
        // 完全なら今までどおり。
        assertEquals(ListingDecision.Replace, Prepare.decideListing(previous, fresh, complete = true))
    }

    @Test fun 不完全なら控えを書かず理由を投げる() {
        val saved = mutableListOf<List<Photo>>()
        val previous = listOf(photo("a"), photo("b"))
        try {
            runBlocking { Prepare.settleListing(previous, Listed(listOf(photo("a")), incomplete = 1)) { saved += it } }
            fail("欠けた一覧で先へ進まない（ハッシュ値の刈り込みまで行ってしまう）")
        } catch (error: IllegalStateException) {
            assertEquals("一部のフォルダを読めませんでした。前の一覧のままです", error.message)
        }
        assertTrue("控えを書き換えない", saved.isEmpty())
    }

    @Test fun 読めなかったフォルダは待てば直りうるが上限は直らない() {
        // 途中のフォルダの時間切れは一時的でありうる → U43 の自動やり直しに乗せる。
        val unreadable = Prepare.incompleteReason(Listed(emptyList(), incomplete = 2), hadPrevious = true)
        assertTrue(Retrying.worthRetrying(Smb.describe(IllegalStateException(unreadable))))
        // 上限は何度歩いても同じ → 網を歩き直さない。
        val capped = Prepare.incompleteReason(Listed(emptyList(), truncated = true), hadPrevious = true)
        assertFalse(Retrying.worthRetrying(Smb.describe(IllegalStateException(capped))))
    }

    @Test fun 完全なら控えを置き換えて返す() {
        val saved = mutableListOf<List<Photo>>()
        val fresh = listOf(photo("c"))
        val got = runBlocking { Prepare.settleListing(listOf(photo("a")), Listed(fresh, incomplete = 0)) { saved += it } }
        assertEquals(fresh, got)
        assertEquals(listOf(fresh), saved)
    }

    @Test fun 組のRAWを外す規則は不完全の印と一緒に残る() {
        val scan = Smb.Scan(
            photos = listOf(
                SmbPhoto("IMG_1.JPG", "d\\IMG_1.JPG", 1, 1),
                SmbPhoto("IMG_1.CR2", "d\\IMG_1.CR2", 1, 1),
                SmbPhoto("IMG_2.CR2", "d\\IMG_2.CR2", 1, 1)
            ),
            unreadable = listOf("d\\x"),
            truncated = false
        )
        val paired = Photos.fromScan("nas1", scan, pairRaw = true)
        assertEquals(listOf("d/IMG_1.JPG", "d/IMG_2.CR2"), paired.photos.map { it.relativePath })
        assertEquals(1, paired.incomplete)
        val all = Photos.fromScan("nas1", scan, pairRaw = false)
        assertEquals(3, all.photos.size)
        assertEquals(SmbRef("nas1", "d\\IMG_1.CR2"), all.photos[1].remote)
    }
}
