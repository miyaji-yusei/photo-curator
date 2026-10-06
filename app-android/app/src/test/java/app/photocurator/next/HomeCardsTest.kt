package app.photocurator.next

import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import java.io.File

class HomeCardsTest {
    // 実機の不具合: 共有の計算を誰も使わないと、ホームのカードが読み込み中のまま固まった。
    @Test fun 共有の計算を誰も使わなくても終わる() = runBlocking {
        var computed = 0
        val result = withTimeout(2_000) {
            withSharedLazy({ computed += 1; 42 }) { _ ->
                (1..5).map { async { it } }.awaitAll().sum()
            }
        }
        assertEquals(15, result)
        assertEquals(0, computed)
    }

    @Test fun 共有の計算は使う人が何人いても1回だけ() = runBlocking {
        var computed = 0
        val result = withTimeout(2_000) {
            withSharedLazy({ computed += 1; 7 }) { shared ->
                (1..5).map { async { shared() } }.awaitAll().sum()
            }
        }
        assertEquals(35, result)
        assertEquals(1, computed)
    }

    @Test fun 署名が同じなら作り直さず違えば作り直す() = runBlocking {
        val memo = Memo<String, List<Long>, Int>()
        var computed = 0
        val compute: suspend () -> Int = { computed += 1; computed }
        assertEquals(1, memo.get("a", listOf(1L), compute))
        assertEquals(1, memo.get("a", listOf(1L), compute))
        assertEquals(1, computed)
        assertEquals(2, memo.get("a", listOf(2L), compute))
        // 別のキーは別
        assertEquals(3, memo.get("b", listOf(2L), compute))
        memo.forget("a")
        assertEquals(4, memo.get("a", listOf(2L), compute))
    }

    @Test fun 表示用画像の名前から置き場と大きさを取り出す() {
        val name = "nas1_${CacheName.of("x")}_1536.jpg"
        assertEquals("nas1" to 1536, CacheName.parseRender(name))
        // 置き場の id に _ が入っていても取れる
        assertEquals("a_b" to 768, CacheName.parseRender("a_b_${CacheName.of("y")}_768.jpg"))
        // 旧名・サムネイル・一時ファイルは数えない
        assertNull(CacheName.parseRender("nas1_1a2b3c4d_1536.jpg"))
        assertNull(CacheName.parseRender("nas1_${CacheName.of("x")}.jpg"))
        assertNull(CacheName.parseRender(name + ".1.writing"))
    }

    @Test fun ファイルの署名は更新と削除で変わる() {
        val dir = kotlin.io.path.createTempDirectory("stamp").toFile()
        try {
            val file = File(dir, "a.json")
            val none = HomeCards.stamp(file)
            file.writeText("1")
            file.setLastModified(1_000_000L)
            val one = HomeCards.stamp(file)
            file.writeText("12")
            val two = HomeCards.stamp(file)
            assert(none != one && one != two)
            file.delete()
            assertEquals(none, HomeCards.stamp(file))
        } finally {
            dir.deleteRecursively()
        }
    }

    // B7: 置き場の id は NAS ごと。同じ NAS の別プロジェクトの絵を足さない。
    private fun nasPhoto(path: String) = Photo(
        id = 1, name = path.substringAfterLast('/'), relativePath = path, size = 1, takenAt = 1,
        remote = SmbRef("nas1", path)
    )

    @Test fun 表示用画像の名前から道筋の名前も取り出す() {
        val hash = CacheName.of("a/b.jpg")
        assertEquals(Triple("nas1", hash, 1536), CacheName.parseRenderParts("nas1_${hash}_1536.jpg"))
        assertEquals(Triple("a_b", hash, 768), CacheName.parseRenderParts("a_b_${hash}_768.jpg"))
        assertNull(CacheName.parseRenderParts("nas1_1a2b3c4d_1536.jpg"))
        assertNull(CacheName.parseRenderParts("nas1_${hash}_1536.jpg.1.writing"))
    }

    @Test fun 表示用画像の数はプロジェクトの写真の顔ぶれで数える() {
        // 同じ NAS の 2 プロジェクト。置き場には両方の絵が入っている。
        val mine = listOf(nasPhoto("a/1.jpg"), nasPhoto("a/2.jpg"), nasPhoto("a/3.jpg"))
        val others = listOf("b/1.jpg", "b/2.jpg", "b/3.jpg", "b/4.jpg").map(CacheName::of)
        val onDisk = (listOf(CacheName.of("a/1.jpg")) + others).toSet()
        assertEquals(1, Renders.madeCount(mine, onDisk))
        assertEquals(0, Renders.madeCount(mine, emptySet()))
        assertEquals(3, Renders.madeCount(mine, mine.map { CacheName.of(it.relativePath) }.toSet() + others))
    }

    @Test fun 端末の写真は表示用画像の道筋を持たないので数えない() {
        val local = Photo(id = 1, name = "x.jpg", relativePath = "x.jpg", size = 1, takenAt = 1)
        assertNull(Renders.pathOf(local))
        assertEquals(0, Renders.madeCount(listOf(local), setOf(CacheName.of("x.jpg"))))
    }
}
