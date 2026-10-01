package app.photocurator.next

import org.junit.Assert.assertEquals
import org.junit.Test

class PrepareOrderTest {
    private fun photo(rel: String, at: Long) =
        Photo(id = rel.hashCode().toLong(), name = rel, relativePath = rel, size = 1, takenAt = at)

    @Test fun 撮影時刻の昇順で同時刻は相対パス順() {
        // NAS は名前順で来る。撮影時刻は名前と逆に並んでいる。
        val listed = listOf(photo("a.jpg", 300), photo("b.jpg", 100), photo("d.jpg", 200), photo("c.jpg", 200))
        val ordered = Prepare.inShootingOrder(listed).map { it.relativePath }
        assertEquals(listOf("b.jpg", "c.jpg", "d.jpg", "a.jpg"), ordered)
    }

    @Test fun すでに昇順なら変わらず空でも落ちない() {
        val sorted = listOf(photo("a", 1), photo("b", 2))
        assertEquals(sorted, Prepare.inShootingOrder(sorted))
        assertEquals(emptyList<Photo>(), Prepare.inShootingOrder(emptyList()))
    }

    @Test fun 選別の並びと同じ規則() {
        val listed = listOf(photo("z", 5), photo("y", 5), photo("x", 1))
        assertEquals(
            listed.sortedWith(compareBy({ it.takenAt }, { it.relativePath })),
            Prepare.inShootingOrder(listed)
        )
    }
}
