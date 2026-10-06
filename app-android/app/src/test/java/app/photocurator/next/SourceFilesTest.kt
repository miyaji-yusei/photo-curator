package app.photocurator.next

import app.photocurator.next.SourceFiles.Kind
import app.photocurator.next.SourceFiles.Outcome
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.nio.file.Files

/**
 * B2: 出所ごとの控え（顔ぶれ・ハッシュ値・つまずき）のファイル名が衝突しないこと、
 * 古い名前の控えを**確かめられたときだけ**引き継ぎ、古いファイルを消さないこと。
 */
class SourceFilesTest {
    private val travel = "nas1|写真/旅行"
    private val family = "nas1|写真/家族"

    private fun dir(): File = Files.createTempDirectory("source-files").toFile()

    /** NAS の 1 枚ぶんの控えの行（[Listing] の形）。 */
    private fun nasEntry(folder: String, name: String, size: Long = 10, nas: String = "nas1") =
        org.json.JSONObject()
            .put("id", name.hashCode().toLong())
            .put("name", name)
            .put("rel", "$folder/$name")
            .put("size", size)
            .put("at", 1L)
            .put("nas", nas)
            .put("path", folder.replace('/', '\\') + "\\" + name)

    private fun listingText(vararg entries: org.json.JSONObject) =
        org.json.JSONArray().apply { entries.forEach { put(it) } }.toString()

    private fun printsText(vararg prints: Pair<String, Long>) =
        org.json.JSONObject().apply {
            for ((path, size) in prints) put(path, org.json.JSONObject().put("v", 1).put("size", size).put("h", "ab"))
        }.toString()

    private fun adopt(dir: File, key: String, known: List<String>, writing: Set<Kind> = emptySet()) =
        runBlocking { SourceFiles.adoptIn(dir, key, writing) { known } }

    // ---- 名前 ----

    @Test fun 文字数が同じ日本語のフォルダ2つは別の名前になる() {
        val d = dir()
        for (kind in Kind.entries) {
            // 古い名前は同じになっていた（これが B2）。
            assertEquals(SourceFiles.legacy(d, kind, travel).name, SourceFiles.legacy(d, kind, family).name)
            // 新しい名前は別。
            assertNotEquals(SourceFiles.current(d, kind, travel).name, SourceFiles.current(d, kind, family).name)
            // 同じ鍵なら同じ名前。
            assertEquals(SourceFiles.current(d, kind, travel), SourceFiles.current(d, kind, travel))
            assertTrue(SourceFiles.current(d, kind, travel).name.startsWith(kind.prefix))
            assertTrue(SourceFiles.current(d, kind, travel).name.endsWith(kind.suffix))
        }
    }

    @Test fun 古い名前を共有しえない鍵は形から分かる() {
        assertTrue(SourceFiles.legacyOnlyMine("12345"))
        assertTrue(SourceFiles.legacyOnlyMine("-12345"))
        assertFalse(SourceFiles.legacyOnlyMine("a_b"))
        assertFalse(SourceFiles.legacyOnlyMine(travel))
        assertTrue(SourceFiles.legacyShared(travel, listOf(travel, family)))
        assertFalse(SourceFiles.legacyShared(travel, listOf(travel, "nas1|写真/旅行先")))
    }

    @Test fun フォルダの中かどうか() {
        assertTrue(SourceFiles.insideFolder("写真\\旅行\\a.jpg", "写真/旅行", deep = false))
        assertFalse(SourceFiles.insideFolder("写真\\旅行\\raw\\a.cr2", "写真/旅行", deep = false))
        assertTrue(SourceFiles.insideFolder("写真\\旅行\\raw\\a.cr2", "写真/旅行", deep = true))
        assertFalse(SourceFiles.insideFolder("写真\\家族\\a.jpg", "写真/旅行", deep = true))
        assertFalse(SourceFiles.insideFolder("写真\\旅行先\\a.jpg", "写真/旅行", deep = true))
        assertTrue(SourceFiles.insideFolder("a.jpg", "", deep = false))
    }

    // ---- 顔ぶれの引き継ぎ ----

    @Test fun 古い顔ぶれが1出所ぶんだけなら引き継ぎ古いファイルは残す() {
        val d = dir()
        val text = listingText(nasEntry("写真/旅行", "a.jpg"), nasEntry("写真/旅行", "b.jpg"))
        val old = SourceFiles.legacy(d, Kind.Listing, travel).apply { writeText(text) }

        val outcome = adopt(d, travel, listOf(travel))

        assertEquals(Outcome.Adopted, outcome[Kind.Listing])
        assertEquals(text, SourceFiles.current(d, Kind.Listing, travel).readText())
        assertTrue("古いファイルは消さない", old.exists())
        assertEquals(text, old.readText())
    }

    @Test fun 別のフォルダが書いた古い顔ぶれは引き継がない() {
        val d = dir()
        // 旅行と家族が同じ古いファイルを使っていて、最後に書いたのは家族。
        val text = listingText(nasEntry("写真/家族", "a.jpg"))
        val old = SourceFiles.legacy(d, Kind.Listing, travel).apply { writeText(text) }

        val forTravel = adopt(d, travel, listOf(travel, family))
        assertEquals(Outcome.Rejected, forTravel[Kind.Listing])
        assertFalse(SourceFiles.current(d, Kind.Listing, travel).exists())
        assertTrue("使わなくても古いファイルは消さない", old.exists())

        // 家族のほうは、中身が家族のものなので引き継ぐ。
        val forFamily = adopt(d, family, listOf(travel, family))
        assertEquals(Outcome.Adopted, forFamily[Kind.Listing])
        assertEquals(text, SourceFiles.current(d, Kind.Listing, family).readText())
        assertTrue(old.exists())
    }

    @Test fun 別のNASや入れ子の行が混ざっていたら引き継がない() {
        val d = dir()
        SourceFiles.legacy(d, Kind.Listing, travel)
            .writeText(listingText(nasEntry("写真/旅行", "a.jpg"), nasEntry("写真/旅行", "b.jpg", nas = "nas2")))
        assertEquals(Outcome.Rejected, adopt(d, travel, listOf(travel))[Kind.Listing])

        val e = dir()
        SourceFiles.legacy(e, Kind.Listing, travel)
            .writeText(listingText(nasEntry("写真/旅行", "a.jpg"), nasEntry("写真/旅行/raw", "b.cr2")))
        assertEquals(Outcome.Rejected, adopt(e, travel, listOf(travel))[Kind.Listing])
        // 「以下ぜんぶ」の鍵なら入れ子もこの出所。
        val deep = "$travel|**"
        val f = dir()
        SourceFiles.legacy(f, Kind.Listing, deep)
            .writeText(listingText(nasEntry("写真/旅行", "a.jpg"), nasEntry("写真/旅行/raw", "b.cr2")))
        assertEquals(Outcome.Adopted, adopt(f, deep, listOf(deep))[Kind.Listing])
    }

    @Test fun 空の顔ぶれと読めない顔ぶれは引き継がない() {
        val d = dir()
        SourceFiles.legacy(d, Kind.Listing, travel).writeText("[]")
        assertEquals(Outcome.Rejected, adopt(d, travel, listOf(travel))[Kind.Listing])
        val e = dir()
        SourceFiles.legacy(e, Kind.Listing, travel).writeText("[ 壊れた")
        assertEquals(Outcome.Rejected, adopt(e, travel, listOf(travel))[Kind.Listing])
    }

    @Test fun 端末のアルバムは古い名前を共有しえないときだけ引き継ぐ() {
        val album = org.json.JSONObject().put("id", 1L).put("name", "a.jpg").put("rel", "DCIM/a.jpg")
            .put("size", 10L).put("at", 1L)
        val d = dir()
        SourceFiles.legacy(d, Kind.Listing, "12345").writeText(listingText(album))
        assertEquals(Outcome.Adopted, adopt(d, "12345", listOf("12345"))[Kind.Listing])

        // `_` を含む鍵は、同じ古い名前になる鍵が端末に無いときだけ。
        val e = dir()
        SourceFiles.legacy(e, Kind.Listing, "a_b").writeText(listingText(album))
        assertEquals(Outcome.Rejected, adopt(e, "a_b", listOf("a_b", "a|b"))[Kind.Listing])
        val f = dir()
        SourceFiles.legacy(f, Kind.Listing, "a_b").writeText(listingText(album))
        assertEquals(Outcome.Adopted, adopt(f, "a_b", listOf("a_b"))[Kind.Listing])
    }

    @Test fun Amazonは共有リンクの鍵で確かめる() {
        val key = "www.amazon.co.jp|ABCdef123"
        fun entry(share: String) = org.json.JSONObject().put("id", 1L).put("name", "a.jpg").put("rel", "node1")
            .put("size", 10L).put("at", 1L).put("amz", share).put("node", "node1").put("tl", "")
        val d = dir()
        SourceFiles.legacy(d, Kind.Listing, key).writeText(listingText(entry(key)))
        assertEquals(Outcome.Adopted, adopt(d, key, listOf(key))[Kind.Listing])
        val e = dir()
        SourceFiles.legacy(e, Kind.Listing, key).writeText(listingText(entry("www.amazon.co.jp|ABCdef124")))
        assertEquals(Outcome.Rejected, adopt(e, key, listOf(key))[Kind.Listing])
    }

    // ---- ハッシュ値の引き継ぎ ----

    @Test fun ハッシュ値は顔ぶれと重なり大きさが合えば引き継ぐ() {
        val d = dir()
        SourceFiles.legacy(d, Kind.Listing, travel)
            .writeText(listingText(nasEntry("写真/旅行", "a.jpg", 10), nasEntry("写真/旅行", "b.jpg", 20)))
        // 家族の分が残っていても（重ならない行）、旅行の分の大きさが合えば旅行のもの。
        val prints = printsText("写真/旅行/a.jpg" to 10, "写真/家族/x.jpg" to 99)
        val old = SourceFiles.legacy(d, Kind.Fingerprints, travel).apply { writeText(prints) }

        val outcome = adopt(d, travel, listOf(travel))

        assertEquals(Outcome.Adopted, outcome[Kind.Listing])
        assertEquals(Outcome.Adopted, outcome[Kind.Fingerprints])
        assertEquals(prints, SourceFiles.current(d, Kind.Fingerprints, travel).readText())
        assertTrue(old.exists())
    }

    @Test fun ハッシュ値は重ならないか大きさが違えば引き継がない() {
        val d = dir()
        SourceFiles.current(d, Kind.Listing, travel).writeText(listingText(nasEntry("写真/旅行", "a.jpg", 10)))
        SourceFiles.legacy(d, Kind.Fingerprints, travel).writeText(printsText("写真/家族/a.jpg" to 10))
        assertEquals(Outcome.Rejected, adopt(d, travel, listOf(travel, family))[Kind.Fingerprints])

        val e = dir()
        SourceFiles.current(e, Kind.Listing, travel).writeText(listingText(nasEntry("写真/旅行", "a.jpg", 10)))
        SourceFiles.legacy(e, Kind.Fingerprints, travel).writeText(printsText("写真/旅行/a.jpg" to 11))
        assertEquals(Outcome.Rejected, adopt(e, travel, listOf(travel))[Kind.Fingerprints])
        assertTrue(SourceFiles.legacy(e, Kind.Fingerprints, travel).exists())
    }

    @Test fun 顔ぶれがまだ無ければハッシュ値は決めずに待ち作り直した顔ぶれで確かめる() {
        val d = dir()
        // 古い顔ぶれは家族のもの（使わない）。ハッシュ値には旅行の分がある。
        SourceFiles.legacy(d, Kind.Listing, travel).writeText(listingText(nasEntry("写真/家族", "a.jpg")))
        SourceFiles.legacy(d, Kind.Fingerprints, travel).writeText(printsText("写真/旅行/a.jpg" to 10))

        val first = adopt(d, travel, listOf(travel, family))
        assertEquals(Outcome.Rejected, first[Kind.Listing])
        assertFalse("まだ決めない", Kind.Fingerprints in first)
        assertFalse(Kind.Fingerprints in SourceFiles.decided(d, travel))

        // 作り直した顔ぶれが書かれた（保存は「顔ぶれを書く」として先に決めてから書く）。
        adopt(d, travel, listOf(travel, family), writing = setOf(Kind.Listing))
        SourceFiles.current(d, Kind.Listing, travel).writeText(listingText(nasEntry("写真/旅行", "a.jpg", 10)))

        val second = adopt(d, travel, listOf(travel, family))
        assertEquals(Outcome.Adopted, second[Kind.Fingerprints])
        assertEquals(Kind.entries.toSet(), SourceFiles.decided(d, travel))
    }

    @Test fun アルバムのハッシュ値は顔ぶれが無くても引き継ぐ() {
        val d = dir()
        SourceFiles.legacy(d, Kind.Fingerprints, "12345").writeText(printsText("DCIM/a.jpg" to 10))
        assertEquals(Outcome.Adopted, adopt(d, "12345", listOf("12345"))[Kind.Fingerprints])
    }

    // ---- つまずき ----

    @Test fun つまずきは古い名前を共有する出所が無いときだけ引き継ぐ() {
        val d = dir()
        SourceFiles.legacy(d, Kind.Trouble, travel).writeText("NAS に届きません")
        assertEquals(Outcome.Rejected, adopt(d, travel, listOf(travel, family))[Kind.Trouble])
        assertTrue(SourceFiles.legacy(d, Kind.Trouble, travel).exists())

        val e = dir()
        SourceFiles.legacy(e, Kind.Trouble, travel).writeText("NAS に届きません")
        assertEquals(Outcome.Adopted, adopt(e, travel, listOf(travel))[Kind.Trouble])
        assertEquals("NAS に届きません", SourceFiles.current(e, Kind.Trouble, travel).readText())
    }

    // ---- 一度決めたら ----

    @Test fun 引き継いだ控えを消してもよみがえらない() {
        val d = dir()
        val text = listingText(nasEntry("写真/旅行", "a.jpg"))
        SourceFiles.legacy(d, Kind.Listing, travel).writeText(text)
        SourceFiles.legacy(d, Kind.Trouble, travel).writeText("NAS に届きません")
        adopt(d, travel, listOf(travel))
        assertEquals(Kind.entries.toSet(), SourceFiles.decided(d, travel))

        // 準備が通ってつまずきを消した・顔ぶれを消した。
        SourceFiles.current(d, Kind.Trouble, travel).delete()
        SourceFiles.current(d, Kind.Listing, travel).delete()

        assertTrue(adopt(d, travel, listOf(travel)).isEmpty())
        assertFalse(SourceFiles.current(d, Kind.Trouble, travel).exists())
        assertFalse(SourceFiles.current(d, Kind.Listing, travel).exists())
        assertTrue(SourceFiles.legacy(d, Kind.Trouble, travel).exists())
    }

    @Test fun 新しい名前があれば古いファイルで上書きしない() {
        val d = dir()
        SourceFiles.legacy(d, Kind.Listing, travel).writeText(listingText(nasEntry("写真/旅行", "old.jpg")))
        val fresh = listingText(nasEntry("写真/旅行", "new.jpg"))
        SourceFiles.current(d, Kind.Listing, travel).writeText(fresh)
        assertEquals(Outcome.Nothing, adopt(d, travel, listOf(travel))[Kind.Listing])
        assertEquals(fresh, SourceFiles.current(d, Kind.Listing, travel).readText())
    }

    @Test fun これから書く種類は引き継がない() {
        val d = dir()
        SourceFiles.legacy(d, Kind.Listing, travel).writeText(listingText(nasEntry("写真/旅行", "a.jpg")))
        val outcome = adopt(d, travel, listOf(travel), writing = setOf(Kind.Listing))
        assertEquals(Outcome.Nothing, outcome[Kind.Listing])
        assertFalse(SourceFiles.current(d, Kind.Listing, travel).exists())
        assertTrue(Kind.Listing in SourceFiles.decided(d, travel))
    }

    @Test fun 控えの読み書きの形は変わらない() {
        val text = listingText(nasEntry("写真/旅行", "a.jpg", 10))
        val photos = Listing.parse(text)
        assertEquals("写真/旅行/a.jpg", photos.single().relativePath)
        assertEquals(SmbRef("nas1", "写真\\旅行\\a.jpg"), photos.single().remote)
        val prints = Fingerprints.parse(printsText("写真/旅行/a.jpg" to 10))
        assertEquals(Fingerprint(1, 10, "ab"), prints["写真/旅行/a.jpg"])
    }
}
