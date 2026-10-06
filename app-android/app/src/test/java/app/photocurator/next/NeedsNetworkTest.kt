package app.photocurator.next

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * U43: ハッシュ値がそろっている NAS のプロジェクトは、開いても NAS へつながない。
 * U30 の A1 で接続の失敗を「止まっています」に載せるようになったため、作るものが
 * 無いのに毎回つないでいると、NAS・Wi-Fi が起きる前に開いただけで止まって見えた。
 */
class NeedsNetworkTest {
    private fun photo(rel: String, size: Long = 10) =
        Photo(id = 1, name = rel, relativePath = rel, size = size, takenAt = 1)

    private fun print(size: Long = 10) = Fingerprint(Analyse.VERSION, size, "abcd", 1)

    @Test fun そろっていればつながない() {
        val photos = listOf(photo("a"), photo("b"))
        assertFalse(Prepare.needsNetwork(photos, mapOf("a" to print(), "b" to print())))
        assertFalse(Prepare.needsNetwork(emptyList(), emptyMap()))
    }

    @Test fun 足りないか変わった写真があればつなぐ() {
        assertTrue(Prepare.needsNetwork(listOf(photo("a"), photo("b")), mapOf("a" to print())))
        assertTrue(Prepare.needsNetwork(listOf(photo("a", size = 20)), mapOf("a" to print(size = 10))))
    }
}
