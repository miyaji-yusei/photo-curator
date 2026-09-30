package app.photocurator.next

import app.photocurator.next.SlideDecision.Drop
import app.photocurator.next.SlideDecision.Keep
import app.photocurator.next.SlideDecision.Top
import app.photocurator.next.SlideshowGesture.decideThreshold
import app.photocurator.next.SlideshowGesture.dragFeedback
import app.photocurator.next.SlideshowGesture.fitContain
import app.photocurator.next.SlideshowGesture.flyTarget
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

    @Test fun タップは左半分が落とす右半分が残す() {
        assertEquals(Drop, tapDecision(100f, 0f, 1000f))
        assertEquals(Drop, tapDecision(499f, 0f, 1000f))
        assertEquals(Keep, tapDecision(500f, 0f, 1000f))
        assertEquals(Keep, tapDecision(900f, 0f, 1000f))
        assertEquals(Keep, tapDecision(400f, 200f, 400f))
        assertEquals(Drop, tapDecision(399f, 200f, 400f))
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
}
