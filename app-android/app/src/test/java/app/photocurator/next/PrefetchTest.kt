package app.photocurator.next

import org.junit.Assert.assertEquals
import org.junit.Test

class PrefetchTest {
    private val queue = listOf("a", "b", "c", "d", "e", "f", "g", "h")

    @Test
    fun slideshowReadsNextThree() {
        assertEquals(listOf("a", "b", "c"), Prefetch.targets(queue, 1, slideshow = true))
    }

    @Test
    fun slideshowWithShortQueue() {
        assertEquals(listOf("a"), Prefetch.targets(listOf("a"), 1, slideshow = true))
        assertEquals(emptyList<String>(), Prefetch.targets(emptyList(), 1, slideshow = true))
    }

    @Test
    fun tournamentReadsNextGroupOnly() {
        assertEquals(listOf("a", "b", "c", "d"), Prefetch.targets(queue, 4, slideshow = false))
    }

    @Test
    fun tournamentIsCapped() {
        val long = (1..20).map { "p$it" }
        assertEquals(Prefetch.TOURNAMENT_MAX, Prefetch.targets(long, 10, slideshow = false).size)
    }

    @Test
    fun tournamentWithZeroGroupSizeStillSafe() {
        assertEquals(listOf("a"), Prefetch.targets(queue, 0, slideshow = false))
    }
}
