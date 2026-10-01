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

    // 枠 left=0, top=0, 幅 1000, 高さ 800 → 上の帯は y < 200（Web の tapDecision のテストと同じ値）
    private fun tap(x: Float, y: Float) = tapDecision(x, y, 0f, 0f, 1000f, 800f)

    @Test fun タップは左半分が落とす右半分が残す() {
        assertEquals(Drop, tap(100f, 400f))
        assertEquals(Drop, tap(379f, 400f))
        assertEquals(Keep, tap(621f, 400f))
        assertEquals(Keep, tap(900f, 799f))
    }

    // 枠 1000×800 → 中心 (500,400)、±120 × ±96、上の帯は y < 200（Web と同じ値）
    @Test fun ほぼ中心は何もしない() {
        assertEquals(0.12f, SlideshowGesture.CENTER_RATIO, eps)
        assertNull(tap(500f, 400f))
        assertNull(tap(450f, 350f))
        assertNull(tap(550f, 450f))
    }

    @Test fun 中心の境目は含み1px外は左右で分ける() {
        assertNull(tap(380f, 400f))
        assertNull(tap(620f, 400f))
        assertEquals(Drop, tap(379f, 400f))
        assertEquals(Keep, tap(621f, 400f))
        assertNull(tap(500f, 304f))
        assertNull(tap(500f, 496f))
        assertEquals(Keep, tap(500f, 497f))
        assertEquals(Drop, tap(499f, 497f))
        assertEquals(Keep, tap(500f, 303f))
        assertEquals(Drop, tap(499f, 303f))
    }

    @Test fun 帯が優先で中心の長方形は帯の外() {
        assertEquals(Top, tap(500f, 199f))
        assertEquals(Keep, tap(500f, 200f))
        // 低い枠では中心の長方形が帯にかかる。そのときも帯が先
        assertEquals(Top, tapDecision(500f, 9f, 0f, 0f, 1000f, 40f))
        assertNull(tapDecision(500f, 16f, 0f, 0f, 1000f, 40f))
    }

    @Test fun 中心は枠がずれていても枠の中心を基準にする() {
        assertNull(tapDecision(400f, 300f, 200f, 100f, 400f, 400f))
        assertNull(tapDecision(448f, 300f, 200f, 100f, 400f, 400f))
        assertEquals(Keep, tapDecision(449f, 300f, 200f, 100f, 400f, 400f))
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

    @Test fun 上の帯は左右を問わず星5() {
        assertEquals(Top, tap(100f, 0f))
        assertEquals(Top, tap(900f, 199f))
        assertEquals(Top, tap(500f, 100f))
    }

    @Test fun 帯の境目から下は左右で分ける() {
        assertEquals(Drop, tap(100f, 200f))
        assertEquals(Keep, tap(900f, 200f))
    }

    @Test fun 枠がずれていても枠を基準にする() {
        assertEquals(Top, tapDecision(400f, 150f, 200f, 100f, 400f, 400f))
        assertEquals(Keep, tapDecision(400f, 200f, 200f, 100f, 400f, 400f))
        assertEquals(Drop, tapDecision(399f, 200f, 200f, 100f, 400f, 400f))
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

    // 幅 411dp・高さ 781dp（Pixel 8 相当のステージ）の中心の範囲は 約 99 × 187dp
    @Test fun 中心の範囲をdpで確かめる() {
        val w = 411f
        val h = 781f
        val halfW = w * SlideshowGesture.CENTER_RATIO
        val halfH = h * SlideshowGesture.CENTER_RATIO
        assertEquals(98.64f, halfW * 2, 0.01f)
        assertEquals(187.44f, halfH * 2, 0.01f)
        assertNull(tapDecision(w / 2 + halfW - 0.01f, h / 2, 0f, 0f, w, h))
        assertEquals(Keep, tapDecision(w / 2 + halfW + 0.01f, h / 2, 0f, 0f, w, h))
        assertNull(tapDecision(w / 2, h / 2 + halfH - 0.01f, 0f, 0f, w, h))
    }
}
