package app.photocurator.next

import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.photo_curator_core.FocusRegion

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

    private fun region(x: Float, y: Float, w: Float, h: Float) = FocusRegion(x, y, w, h, 0.8f)

    @Test
    fun foundOfKeepsEmptyAsNoneButRejectsUnreadableSize() {
        // 四角が 0 個は「くっきりした所なし」。失敗（null）ではない。
        val none = FocusPeaking.foundOf(1024, 683, emptyList())!!
        assertTrue(none.regions.isEmpty())
        assertEquals(1024, none.width)
        assertNull(FocusPeaking.foundOf(0, 683, emptyList()))
        assertNull(FocusPeaking.foundOf(1024, 0, listOf(region(0f, 0f, 0.5f, 0.5f))))
    }

    @Test
    fun foundOfDropsOutOfRangeAndLimitsToThree() {
        val good = region(0.1f, 0.1f, 0.2f, 0.2f)
        val found = FocusPeaking.foundOf(
            100, 100,
            listOf(good, region(0.9f, 0.9f, 0.5f, 0.5f), region(-0.1f, 0f, 0.2f, 0.2f), region(0f, 0f, 0f, 0.2f), good, good, good)
        )!!
        assertEquals(3, found.regions.size)
        assertTrue(found.regions.all { it == good })
    }

    @Test
    fun fitRectLetterboxesLikeContentScaleFit() {
        // 横長の写真（2:1）を 100x100 の箱に: 幅いっぱい、上下に余白。
        val wide = FocusPeaking.fitRect(100f, 100f, 200, 100)
        assertEquals(listOf(0f, 25f, 100f, 50f), wide.toList())
        // 縦長（1:2）: 高さいっぱい、左右に余白。
        val tall = FocusPeaking.fitRect(100f, 100f, 100, 200)
        assertEquals(listOf(25f, 0f, 50f, 100f), tall.toList())
        // 拡大はしない（箱が小さい前提の縮小のみ。ここでは箱に合わせて拡縮する Fit と同じ計算）。
        assertEquals(listOf(0f, 0f, 0f, 0f), FocusPeaking.fitRect(0f, 100f, 200, 100).toList())
        assertEquals(listOf(0f, 0f, 0f, 0f), FocusPeaking.fitRect(100f, 100f, 0, 100).toList())
    }

    @Test
    fun bracketSegmentsDrawOnlyTheFourCorners() {
        // 箱 200x100 に 2:1 の写真 → 範囲は (0,0,200,100)。四角 (0.1,0.2)-(0.6,0.8) = (20,20)-(120,80)、短辺 60 の 30 % = 腕 18。
        val fit = FocusPeaking.fitRect(200f, 100f, 200, 100)
        val segments = FocusPeaking.bracketSegments(region(0.1f, 0.2f, 0.5f, 0.6f), fit)
        assertEquals(8, segments.size)
        fun near(s: BracketSegment, x1: Float, y1: Float, x2: Float, y2: Float) {
            assertEquals(x1, s.x1, 0.01f); assertEquals(y1, s.y1, 0.01f)
            assertEquals(x2, s.x2, 0.01f); assertEquals(y2, s.y2, 0.01f)
        }
        near(segments[0], 20f, 20f, 38f, 20f)
        near(segments[1], 20f, 20f, 20f, 38f)
        near(segments[7], 120f, 80f, 120f, 62f)
        // 辺の真ん中は描かない: どの線も端点が四角の角にある。
        for (s in segments) {
            assertTrue(Math.abs(s.x1 - 20f) < 0.01f || Math.abs(s.x1 - 120f) < 0.01f)
            assertTrue(Math.abs(s.y1 - 20f) < 0.01f || Math.abs(s.y1 - 80f) < 0.01f)
        }
    }

    @Test
    fun bracketStyleMakesFirstBoldAndOthersThinAndFaint() {
        val first = FocusPeaking.bracketStyle(0)
        val second = FocusPeaking.bracketStyle(1)
        assertTrue(first.widthDp > second.widthDp)
        assertTrue(first.alpha > second.alpha)
        assertEquals(second, FocusPeaking.bracketStyle(2))
    }

    @Test
    fun noneLabelsMatchWebAndCompactIsShorter() {
        assertEquals("くっきりした所が見つかりません", FocusPeaking.NONE_LABEL)
        assertEquals("ピント甘め？", FocusPeaking.NONE_LABEL_COMPACT)
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

    private val alpha = FocusFound(1, 1, emptyList())

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
