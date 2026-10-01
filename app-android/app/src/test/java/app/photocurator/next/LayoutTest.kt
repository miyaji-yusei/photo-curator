package app.photocurator.next

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class LayoutTest {
    @Test
    fun カバー画面の幅は狭い() {
        assertTrue(isNarrowWidth(360))
        assertTrue(isNarrowWidth(619))
    }

    @Test
    fun 開いた幅と620dpちょうどは広い() {
        assertFalse(isNarrowWidth(620))
        assertFalse(isNarrowWidth(900))
    }
}
