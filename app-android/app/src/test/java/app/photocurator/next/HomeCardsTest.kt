package app.photocurator.next

import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import java.io.File

class HomeCardsTest {
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
