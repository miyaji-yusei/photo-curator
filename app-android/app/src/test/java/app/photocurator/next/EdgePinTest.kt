package app.photocurator.next

import org.junit.Assert.assertEquals
import org.junit.Test

class EdgePinTest {
    @Test fun 自分の大きさを持たないプロジェクトだけ書き留める() {
        val own = setOf("p2")
        assertEquals(listOf("p1", "p3"), Prefs.edgesToPin(listOf("p1", "p2", "p3")) { it in own })
    }

    @Test fun 全員が持っていれば何も書かない() {
        assertEquals(emptyList<String>(), Prefs.edgesToPin(listOf("p1")) { true })
        assertEquals(emptyList<String>(), Prefs.edgesToPin(emptyList()) { false })
    }
}
