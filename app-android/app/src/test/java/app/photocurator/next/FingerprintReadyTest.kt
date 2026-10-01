package app.photocurator.next

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class FingerprintReadyTest {
    private fun photo(rel: String, size: Long = 10) =
        Photo(id = 1, name = rel, relativePath = rel, size = size, takenAt = 1)

    private fun print(size: Long = 10, version: Int = Analyse.VERSION, hash: String = "ab") =
        Fingerprint(version, size, hash)

    @Test fun 全部そろっていれば準備済み() {
        val photos = listOf(photo("a"), photo("b"))
        assertTrue(Analyse.allUpToDate(photos, mapOf("a" to print(), "b" to print())))
    }

    @Test fun 作れなかった印の空のハッシュ値もそろっている扱い() {
        assertTrue(Analyse.allUpToDate(listOf(photo("a")), mapOf("a" to print(hash = ""))))
    }

    @Test fun 一枚でも無いか古い版か大きさ違いなら準備済みではない() {
        val photos = listOf(photo("a"), photo("b"))
        assertFalse(Analyse.allUpToDate(photos, mapOf("a" to print())))
        assertFalse(Analyse.allUpToDate(photos, mapOf("a" to print(), "b" to print(version = Analyse.VERSION - 1))))
        assertFalse(Analyse.allUpToDate(photos, mapOf("a" to print(), "b" to print(size = 99))))
    }

    @Test fun 写真が無ければ空の控えは準備済みと言えるが呼び出し側は空を使わない() {
        assertTrue(Analyse.allUpToDate(emptyList(), emptyMap()))
    }
}
