package app.photocurator.next

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class CacheNameTest {
    // 旧実装（32 ビットの hashCode）。衝突の多さを示すために残す。
    private fun legacy(path: String) = path.hashCode().toUInt().toString(16)

    private fun paths(count: Int) = (0 until count).map { "2024/trip/DSC_${it.toString().padStart(6, '0')}.JPG" }

    @Test fun 旧名は2万枚で衝突する組がある() {
        // 20,000 枚で約 4.5%、20 万枚なら期待値で 4〜5 組。固定の入力なので毎回同じ結果。
        // 連番の名前は hashCode が偏って衝突しにくいので、散らばった名前で数える。
        val random = java.util.Random(42)
        val all = (0 until 200_000).map { "dir/" + java.lang.Long.toHexString(random.nextLong()) + ".jpg" }
        assertTrue(all.map(::legacy).toSet().size < all.size)
    }

    @Test fun 新名は2万個で衝突しない() {
        for (seed in 0 until 5) {
            val all = (0 until 20_000).map { "dir$seed/IMG_$it.jpg" }
            assertEquals(all.size, all.map(CacheName::of).toSet().size)
        }
    }

    @Test fun 同じ道筋なら同じ名前で32桁() {
        val a = CacheName.of("a/b.jpg")
        assertEquals(a, CacheName.of("a/b.jpg"))
        assertEquals(32, a.length)
        assertTrue(Regex("[0-9a-f]{32}").matches(a))
    }

    @Test fun 日本語の道筋でも名前になる() {
        assertEquals(32, CacheName.of("旅行/写真 1.jpg").length)
    }

    @Test fun 旧名と新名を見分ける() {
        val newName = "nas1_${CacheName.of("x")}_1024.jpg"
        assertTrue(CacheName.isRender(newName, "nas1", 1024))
        assertFalse(CacheName.isRender(newName, "nas1", 768))
        assertTrue(CacheName.isCurrent(newName))
        assertTrue(CacheName.isCurrent("nas1_${CacheName.of("x")}.jpg"))
        assertFalse(CacheName.isCurrent("nas1_1a2b3c4d_1024.jpg"))
        assertFalse(CacheName.isCurrent("nas1_1a2b3c4d.jpg"))
        assertFalse(CacheName.isRender("nas1_1a2b3c4d_1024.jpg", "nas1", 1024))
    }
}
