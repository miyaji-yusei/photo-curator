package app.photocurator.next

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test

class PartialCounterTest {
    @Test fun 変わらないものだけなら一度も保存しない() {
        val counter = PartialCounter(50)
        assertEquals(0, (1..2000).count { counter.add(false) })
    }

    @Test fun 変わった分が50件たまるごとに保存する() {
        val counter = PartialCounter(50)
        assertEquals(4, (1..200).count { counter.add(true) })
    }

    @Test fun 変わらないものが挟まっても数えない() {
        val counter = PartialCounter(3)
        val flushed = ArrayList<Int>()
        listOf(true, false, false, true, false, true, true).forEachIndexed { at, changed ->
            if (counter.add(changed)) flushed += at
        }
        assertEquals(listOf(5), flushed)
        assertFalse(counter.add(false))
    }
}
