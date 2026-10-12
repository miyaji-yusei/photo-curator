package app.photocurator.next

import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

/** U60: ピントの位置。画面に触らない部分（輝度・色付け・キャッシュ・状態）。 */
class FocusPeakingTest {
    private fun argb(r: Int, g: Int, b: Int) = (0xFF shl 24) or (r shl 16) or (g shl 8) or b

    @Test
    fun luminanceUsesBt709() {
        val out = FocusPeaking.luminance(
            intArrayOf(argb(0, 0, 0), argb(255, 255, 255), argb(255, 0, 0), argb(0, 255, 0), argb(0, 0, 255))
        )
        assertEquals(listOf(0, 255, 54, 182, 18), out.map { it.toInt() })
    }

    @Test
    fun luminanceIgnoresAlpha() {
        val out = FocusPeaking.luminance(intArrayOf(0x00FFFFFF, 0x80808080.toInt()))
        assertEquals(listOf(255, 128), out.map { it.toInt() })
    }

    @Test
    fun fitWithinShrinksLongEdgeOnly() {
        assertEquals(1024 to 683, FocusPeaking.fitWithin(6000, 4000, 1024))
        assertEquals(683 to 1024, FocusPeaking.fitWithin(4000, 6000, 1024))
        // 拡大はしない。
        assertEquals(800 to 600, FocusPeaking.fitWithin(800, 600, 1024))
        assertEquals(0 to 0, FocusPeaking.fitWithin(0, 600, 1024))
        assertEquals(0 to 0, FocusPeaking.fitWithin(-1, -1, 1024))
    }

    @Test
    fun alphaOfRejectsEmptyAndMismatched() {
        assertNull(FocusPeaking.alphaOf(0u, 0u, emptyList()))
        assertNull(FocusPeaking.alphaOf(2u, 2u, listOf(1u.toUByte())))
        assertNull(FocusPeaking.alphaOf(0u, 3u, emptyList()))
    }

    @Test
    fun argbCarriesStrengthAsAlphaInLime() {
        val alpha = FocusPeaking.alphaOf(2u, 1u, listOf(0u.toUByte(), 200u.toUByte()))!!
        val px = alpha.toArgb()
        assertEquals(2, alpha.width)
        assertEquals(1, alpha.height)
        assertEquals(0x00D6FF73, px[0])
        assertEquals((200 shl 24) or 0xD6FF73, px[1])
    }

    @Test
    fun cacheEvictsLeastRecentlyUsed() {
        val cache = MaskCache<String, Int>(2)
        cache.put("a", 1)
        cache.put("b", 2)
        assertEquals(1, cache.get("a")) // a を使った → 古いのは b
        cache.put("c", 3)
        assertNull(cache.get("b"))
        assertEquals(1, cache.get("a"))
        assertEquals(3, cache.get("c"))
        assertEquals(2, cache.size)
    }

    private val alpha = FocusAlpha(1, 1, byteArrayOf(9))

    @Test
    fun storeComputesOncePerKey() = runBlocking {
        val store = MaskStore(limit = 5)
        var runs = 0
        val key = MaskKey(1L, "a.jpg", FocusKind.Display)
        val first = store.load(key) { runs += 1; alpha }
        val second = store.load(key) { runs += 1; alpha }
        assertEquals(1, runs)
        assertSame(first, second)
        assertSame(alpha, store.cached(key))
    }

    @Test
    fun storeKeepsDisplayAndOriginalApartAndDoesNotCacheFailure() = runBlocking {
        val store = MaskStore(limit = 5)
        val display = MaskKey(1L, "a.jpg", FocusKind.Display)
        val original = MaskKey(1L, "a.jpg", FocusKind.Original)
        assertNull(store.load(display) { null })
        assertNull(store.cached(display))
        store.load(original) { alpha }
        assertNull(store.cached(display))
        assertSame(alpha, store.cached(original))
    }

    @Test
    fun storeSeparatesPhotosWithSameIdButOtherPath() = runBlocking {
        val store = MaskStore(limit = 5)
        store.load(MaskKey(7L, "x/a.jpg", FocusKind.Display)) { alpha }
        assertNull(store.cached(MaskKey(7L, "y/a.jpg", FocusKind.Display)))
    }

    @Test
    fun toggleIsSharedAndStartsOff() {
        FocusPeaking.enabled.value = false
        assertFalse(FocusPeaking.enabled.value)
        FocusPeaking.toggle()
        assertTrue(FocusPeaking.enabled.value)
        FocusPeaking.toggle()
        assertFalse(FocusPeaking.enabled.value)
    }
}
