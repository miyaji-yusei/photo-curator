package app.photocurator.next

import app.photocurator.next.SlideDecision.Drop
import app.photocurator.next.SlideDecision.Keep
import app.photocurator.next.SlideDecision.Top
import app.photocurator.next.SlideshowGesture.decideThreshold
import app.photocurator.next.SlideshowGesture.dragFeedback
import app.photocurator.next.SlideshowGesture.fitContain
import app.photocurator.next.SlideshowGesture.flyTarget
import app.photocurator.next.SlideshowGesture.TapRecord
import app.photocurator.next.SlideshowGesture.isDoubleTap
import app.photocurator.next.SlideshowGesture.isTap
import app.photocurator.next.SlideshowGesture.judgeDrag
import app.photocurator.next.SlideshowGesture.tapDecision
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** PC・Web の tests/slideshowGesture.test.ts と同じ数値・同じ規則であることを確かめる。 */
class SlideshowGestureTest {
    private val eps = 1e-3f

    @Test fun 閾値は幅の18パーセントで60から160に収める() {
        assertEquals(60f, decideThreshold(200f), eps)
        assertEquals(90f, decideThreshold(500f), eps)
        assertEquals(160f, decideThreshold(2000f), eps)
        assertEquals(60f, decideThreshold(Float.NaN), eps)
    }

    @Test fun 動きが小さければタップ() {
        assertTrue(isTap(0f, 0f))
        assertTrue(isTap(SlideshowGesture.TAP_SLOP - 1, 0f))
        assertTrue(!isTap(SlideshowGesture.TAP_SLOP, 0f))
        assertTrue(!isTap(6f, 6f))
    }

    @Test fun 左は落とす右は残す() {
        assertEquals(Drop, judgeDrag(-90f, 0f, 500f))
        assertEquals(Keep, judgeDrag(120f, 10f, 500f))
    }

    @Test fun 閾値に足りなければ決めない() {
        assertNull(judgeDrag(-89f, 0f, 500f))
        assertNull(judgeDrag(40f, 0f, 500f))
    }

    @Test fun 上へ閾値以上なら星5で確定() {
        assertEquals(Top, judgeDrag(0f, -90f, 500f))
        assertEquals(Top, judgeDrag(30f, -150f, 500f))
        assertNull(judgeDrag(0f, -89f, 500f))
    }

    @Test fun 下向きは使わない() {
        assertNull(judgeDrag(0f, 200f, 500f))
        assertNull(judgeDrag(10f, 400f, 500f))
    }

    @Test fun 斜めは優位な軸で決める_同じなら横() {
        assertEquals(Top, judgeDrag(-100f, -200f, 500f))
        assertEquals(Drop, judgeDrag(-200f, -100f, 500f))
        assertEquals(Keep, judgeDrag(150f, -150f, 500f))
        assertNull(judgeDrag(150f, 200f, 500f))
    }

    @Test fun ドラッグ中の濃さは閾値で1() {
        assertEquals(Keep, dragFeedback(45f, 0f, 500f).direction)
        assertEquals(0.5f, dragFeedback(45f, 0f, 500f).strength, eps)
        assertEquals(Drop, dragFeedback(-90f, 0f, 500f).direction)
        assertEquals(1f, dragFeedback(-90f, 0f, 500f).strength, eps)
        assertEquals(1f, dragFeedback(-900f, 0f, 500f).strength, eps)
    }

    @Test fun 上が優位ならtopで下は何も出さない() {
        assertEquals(Top, dragFeedback(5f, -45f, 500f).direction)
        assertEquals(0.5f, dragFeedback(5f, -45f, 500f).strength, eps)
        assertNull(dragFeedback(0f, 100f, 500f).direction)
        assertEquals(0f, dragFeedback(0f, 100f, 500f).strength, eps)
    }

    @Test fun 回転は横の動きに比例し15度まで() {
        assertEquals(3f, dragFeedback(50f, 0f, 500f).rotation, eps)
        assertEquals(-3f, dragFeedback(-50f, 0f, 500f).rotation, eps)
        assertEquals(15f, dragFeedback(5000f, 0f, 500f).rotation, eps)
        assertEquals(-15f, dragFeedback(-5000f, 0f, 500f).rotation, eps)
    }

    @Test fun 決定になる動きでは判定と同じ向きになる() {
        for ((dx, dy) in listOf(-100f to 0f, 100f to 0f, 0f to -100f, -100f to -200f)) {
            assertEquals(judgeDrag(dx, dy, 500f), dragFeedback(dx, dy, 500f).direction)
        }
    }

    // 枠 left=0, top=0, 幅 1000, 高さ 800 → 上は y < 240、左は x < 300、右は x > 700（Web の tapDecision のテストと同じ値）
    private fun tap(x: Float, y: Float) = tapDecision(x, y, 0f, 0f, 1000f, 800f)

    @Test fun 定数は0点3() {
        assertEquals(0.3, SlideshowGesture.TOP_ZONE_RATIO, 1e-9)
        assertEquals(0.3, SlideshowGesture.SIDE_ZONE_RATIO, 1e-9)
    }

    @Test fun 上の30パーセント未満は左右中央を問わず星5() {
        assertEquals(Top, tap(100f, 0f))
        assertEquals(Top, tap(900f, 239f))
        assertEquals(Top, tap(500f, 100f))
        assertEquals(Top, tap(300f, 239f))
        assertEquals(Top, tap(700f, 239f))
    }

    @Test fun 上の線から下は左右中央で分ける() {
        assertEquals(Drop, tap(100f, 240f))
        assertEquals(Keep, tap(900f, 240f))
        assertNull(tap(500f, 240f))
    }

    @Test fun 左端は落とし右端は残す() {
        assertEquals(Drop, tap(0f, 400f))
        assertEquals(Drop, tap(299f, 400f))
        assertEquals(Keep, tap(701f, 400f))
        assertEquals(Keep, tap(999f, 799f))
        assertEquals(Drop, tap(100f, 799f))
    }

    @Test fun ちょうど30パーセントと70パーセントの線は中央() {
        assertNull(tap(300f, 400f))
        assertNull(tap(700f, 400f))
        assertNull(tap(300f, 799f))
        assertNull(tap(700f, 240f))
    }

    @Test fun 中央の縦帯は上の線の下から最下部まで何もしない() {
        for (y in listOf(240f, 300f, 400f, 500f, 700f, 790f, 799f)) {
            assertNull(tap(500f, y))
            assertNull(tap(301f, y))
            assertNull(tap(699f, y))
        }
    }

    @Test fun 枠がずれていても枠を基準にする() {
        // 枠 x=200..600, y=100..500 → 上は y<220、左は x<320、右は x>480
        assertEquals(Top, tapDecision(400f, 219f, 200f, 100f, 400f, 400f))
        assertEquals(Drop, tapDecision(319f, 300f, 200f, 100f, 400f, 400f))
        assertNull(tapDecision(320f, 300f, 200f, 100f, 400f, 400f))
        assertNull(tapDecision(480f, 499f, 200f, 100f, 400f, 400f))
        assertEquals(Keep, tapDecision(481f, 300f, 200f, 100f, 400f, 400f))
        assertNull(tapDecision(400f, 220f, 200f, 100f, 400f, 400f))
    }

    // 画面側（Slideshow.kt）と同じ手順: null（中央）の結果だけを二度タップの記録に使う
    private fun runTaps(points: List<Triple<Long, Float, Float>>): Boolean {
        var last: TapRecord? = null
        var zoomed = false
        for ((t, x, y) in points) {
            if (tap(x, y) != null) { last = null; continue }
            val now = TapRecord(t, x, y)
            if (isDoubleTap(last, now)) { last = null; zoomed = true } else last = now
        }
        return zoomed
    }

    @Test fun 二度タップの拡大は中央の縦帯のどこでも成立する() {
        assertTrue(runTaps(listOf(Triple(0L, 500f, 245f), Triple(200L, 500f, 250f))))
        assertTrue(runTaps(listOf(Triple(0L, 500f, 400f), Triple(300L, 510f, 405f))))
        assertTrue(runTaps(listOf(Triple(0L, 400f, 790f), Triple(100L, 420f, 795f))))
        assertTrue(runTaps(listOf(Triple(0L, 301f, 500f), Triple(100L, 305f, 500f))))
    }

    @Test fun 左右と上の領域では二度タップしても拡大しない() {
        assertTrue(!runTaps(listOf(Triple(0L, 100f, 500f), Triple(100L, 100f, 500f))))
        assertTrue(!runTaps(listOf(Triple(0L, 900f, 500f), Triple(100L, 900f, 500f))))
        assertTrue(!runTaps(listOf(Triple(0L, 500f, 100f), Triple(100L, 500f, 100f))))
    }

    @Test fun 間に別の領域が挟まれたり遅い遠い2回目は拡大しない() {
        assertTrue(!runTaps(listOf(Triple(0L, 500f, 400f), Triple(50L, 100f, 400f), Triple(100L, 500f, 400f))))
        assertTrue(!runTaps(listOf(Triple(0L, 500f, 400f), Triple(301L, 500f, 400f))))
        assertTrue(!runTaps(listOf(Triple(0L, 400f, 400f), Triple(100L, 430f, 400f))))
    }


    private fun at(time: Long, x: Float = 100f, y: Float = 100f) = TapRecord(time, x, y)

    @Test fun 二度タップの定数() {
        assertEquals(300L, SlideshowGesture.DOUBLE_TAP_MS)
        assertEquals(24f, SlideshowGesture.DOUBLE_TAP_DISTANCE, eps)
    }

    @Test fun 記録が無ければ二度タップでない() {
        assertTrue(!isDoubleTap(null, at(0)))
    }

    @Test fun 二度タップの時間の境目() {
        assertTrue(isDoubleTap(at(1000), at(1100)))
        assertTrue(isDoubleTap(at(1000), at(1300)))
        assertTrue(!isDoubleTap(at(1000), at(1301)))
        assertTrue(!isDoubleTap(at(1000), at(999)))
    }

    @Test fun 二度タップの距離の境目() {
        assertTrue(isDoubleTap(at(0, 100f, 100f), at(100, 124f, 100f)))
        assertTrue(!isDoubleTap(at(0, 100f, 100f), at(100, 125f, 100f)))
        assertTrue(!isDoubleTap(at(0, 100f, 100f), at(100, 118f, 118f)))
        assertTrue(isDoubleTap(at(0, 100f, 100f), at(100, 116f, 116f)))
    }

    @Test fun 飛んでいく先は枠の外() {
        assertTrue(flyTarget(Drop, 1000f, 600f).x < -1000f)
        assertTrue(flyTarget(Keep, 1000f, 600f).x > 1000f)
        assertEquals(0f, flyTarget(Top, 1000f, 600f).x, eps)
        assertTrue(flyTarget(Top, 1000f, 600f).y < -600f)
    }

    @Test fun 縦横比を保って枠いっぱいにする() {
        assertEquals(900f to 600f, fitContain(300f, 200f, 900f, 900f))
        assertEquals(400f to 600f, fitContain(200f, 300f, 900f, 600f))
        assertEquals(50f to 50f, fitContain(10f, 10f, 100f, 50f))
        assertEquals(100f to 50f, fitContain(0f, 0f, 100f, 50f))
    }

    // U29: 実機で「中央を押したのに振り分けられる」と報告された状況。エミュレーターでは再現せず、
    // 純関数の側で「指が少し動いても中心からは決定しない」ことを固定する（幅 411dp の折りたたみ閉じ）。
    @Test fun 指が少し動いても決定にならない() {
        val width = 411f
        // 8dp 以上動いてドラッグ扱いになっても、決定量（幅の 18% = 約 74dp）に届かなければ戻すだけ
        for (d in listOf(8f, 12f, 30f, 60f, 73f)) {
            assertNull(judgeDrag(d, 0f, width))
            assertNull(judgeDrag(-d, d / 2, width))
            assertNull(judgeDrag(0f, -d, width))
        }
        assertTrue(isTap(7.9f, 0f))
        assertTrue(!isTap(8f, 0f))
    }

    // 幅 411dp・高さ 781dp（Pixel 8 相当のステージ）: 上は 約 234dp、左右は 約 123dp ずつ、中央の縦帯は 約 164dp
    @Test fun 領域の範囲をdpで確かめる() {
        val w = 411f
        val h = 781f
        assertEquals(Top, tapDecision(w / 2, h * 0.3f - 0.01f, 0f, 0f, w, h))
        assertNull(tapDecision(w / 2, h * 0.3f, 0f, 0f, w, h))
        assertEquals(Drop, tapDecision(w * 0.3f - 0.01f, h / 2, 0f, 0f, w, h))
        assertNull(tapDecision(w * 0.3f, h / 2, 0f, 0f, w, h))
        assertNull(tapDecision(w * 0.7f, h / 2, 0f, 0f, w, h))
        assertEquals(Keep, tapDecision(w * 0.7f + 0.01f, h / 2, 0f, 0f, w, h))
        assertNull(tapDecision(w / 2, h - 1f, 0f, 0f, w, h))
    }
}
