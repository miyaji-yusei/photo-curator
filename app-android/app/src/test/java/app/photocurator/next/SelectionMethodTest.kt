package app.photocurator.next

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * 方式と枚数の保留値（U24）。Options（選別中の設定）は、確定ボタンを押すまで
 * 実際の枚数を変えないので、その「押すまで」と「押したときの値」をここで確かめる。
 */
class SelectionMethodTest {

    @Test fun `group_size 1 はスライドショー、2 以上はトーナメント`() {
        assertEquals(Method.Slideshow, PendingMethod.from(1, 4).method)
        assertEquals(Method.Tournament, PendingMethod.from(2, 4).method)
        assertEquals(Method.Tournament, PendingMethod.from(10, 4).method)
    }

    @Test fun `トーナメントの保存済みの枚数がそのまま保留値になる`() {
        val pending = PendingMethod.from(6, 4)
        assertEquals(6, pending.tournamentSize)
        assertEquals(6, pending.size)
    }

    @Test fun `スライドショーの保留値は 1 で、戻したときのために直前のトーナメントの枚数を覚えている`() {
        val pending = PendingMethod.from(Prefs.SLIDESHOW_SIZE, 7)
        assertEquals(Prefs.SLIDESHOW_SIZE, pending.size)
        assertEquals(7, pending.tournamentSize)
        // トーナメントに切り替えると、覚えていた枚数に戻る
        assertEquals(7, pending.withMethod(Method.Tournament).size)
    }

    @Test fun `方式を切り替えただけでは枚数の値は変わらず、スライドショーへは 1 になる`() {
        val tournament = PendingMethod.from(5, 5)
        val slideshow = tournament.withMethod(Method.Slideshow)
        assertEquals(1, slideshow.size)
        assertEquals(5, slideshow.tournamentSize)
        // もとの（保存済みの）枚数と比べると、変わったことが分かる
        assertTrue(slideshow.differsFrom(5))
        assertFalse(tournament.differsFrom(5))
    }

    @Test fun `枚数の丸を選ぶとトーナメントになり、2〜10 に収まる`() {
        val fromSlideshow = PendingMethod.from(1, 4).withSize(8)
        assertEquals(Method.Tournament, fromSlideshow.method)
        assertEquals(8, fromSlideshow.size)
        assertEquals(2, PendingMethod.from(4, 4).withSize(0).size)
        assertEquals(10, PendingMethod.from(4, 4).withSize(99).size)
    }

    @Test fun `範囲外の保存値は収める`() {
        assertEquals(2, PendingMethod.from(Prefs.SLIDESHOW_SIZE, 0).tournamentSize)
        assertEquals(10, PendingMethod.from(Prefs.SLIDESHOW_SIZE, 50).tournamentSize)
        assertEquals(10, PendingMethod.from(99, 4).size)
    }

    @Test fun `確定ボタンの文言は PC・Web と同じ`() {
        assertEquals("この方式にする", PendingMethod(Method.Slideshow, 4).confirmLabel)
        assertEquals("この枚数にする", PendingMethod(Method.Tournament, 4).confirmLabel)
    }

    @Test fun `選ぶだけでは保存済みの値は動かない（保留値は別の値）`() {
        // 画面は保存済みの groupSize から保留値を作り、選ぶのは保留値だけ。確定までは groupSize のまま。
        val saved = 4
        var pending = PendingMethod.from(saved, 4)
        pending = pending.withSize(9)
        pending = pending.withMethod(Method.Slideshow)
        assertEquals(4, saved)
        assertEquals(1, pending.size)
        // キャンセルは保留値を捨てるだけなので、もう一度開くと保存済みの値から始まる
        assertEquals(4, PendingMethod.from(saved, 4).size)
    }

    @Test fun `ヘルプはどちらの方式にも節と項目があり、長押しの拡大が無いことを書く`() {
        for (method in Method.values()) {
            val guide = selectionHelp(method)
            assertTrue(guide.sections.isNotEmpty())
            for (section in guide.sections) assertTrue(section.items.isNotEmpty())
        }
        val slideshow = selectionHelp(Method.Slideshow).sections.flatMap { it.items }
        assertTrue(slideshow.any { it.contains("長押しの拡大はありません") })
        assertTrue(slideshow.any { it.contains("中心を二度タップ") })
    }
}
