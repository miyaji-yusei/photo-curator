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
}
