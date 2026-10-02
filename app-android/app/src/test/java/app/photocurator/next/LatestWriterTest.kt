package app.photocurator.next

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.util.concurrent.atomic.AtomicInteger

class LatestWriterTest {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    @Test fun 連打しても最後の値が残り同時には書かない() = runBlocking {
        val inside = AtomicInteger(0)
        var overlapped = false
        var last = -1
        val writer = LatestWriter<Int>(scope, write = { value ->
            if (inside.incrementAndGet() > 1) overlapped = true
            delay(3)
            last = value
            inside.decrementAndGet()
        })
        val waits = (1..200).map { writer.submit(it) }
        waits.awaitAll()
        assertFalse("同時に書いた", overlapped)
        assertEquals(200, last)
    }

    @Test fun 書いている間に頼んだ古い値は飛ばされても最後は勝つ() = runBlocking {
        val written = ArrayList<Int>()
        val writer = LatestWriter<Int>(scope, write = { value -> delay(20); synchronized(written) { written.add(value) } })
        val first = writer.submit(1)
        delay(5)
        writer.submit(2)
        writer.submit(3)
        val last = writer.submit(4)
        first.await(); last.await()
        assertEquals(4, written.last())
        assertTrue(written.size <= 3)
        // 順序は逆転しない
        assertEquals(written.sorted(), written)
    }

    @Test fun 書くのに失敗しても次が書ける() = runBlocking {
        var last = 0
        val writer = LatestWriter<Int>(scope, write = { value ->
            if (value == 1) throw IllegalStateException("x")
            last = value
        })
        writer.submit(1).await()
        writer.submit(2).await()
        assertEquals(2, last)
    }

    @Test fun 保存の列を通せば8本から同時に頼んでも読めるファイルが残り最後が勝つ() = runBlocking {
        val dir = kotlin.io.path.createTempDirectory("atomic").toFile()
        try {
            val target = File(dir, "session.json")
            val failures = java.util.concurrent.ConcurrentLinkedQueue<Exception>()
            val writer = LatestWriter<Int>(scope, write = { value ->
                target.writeAtomically { it.writeText("{\"v\":$value}") }
            }, onError = { failures.add(it) })
            val threads = (1..8).map { index ->
                Thread { repeat(200) { round -> writer.submit(index * 1000 + round) } }
            }
            threads.forEach { it.start() }
            threads.forEach { it.join() }
            writer.submit(999999).await()
            assertTrue("書き込みの失敗: $failures", failures.isEmpty())
            assertEquals("{\"v\":999999}", target.readText())
            assertEquals(listOf("session.json"), dir.list()!!.toList())
        } finally {
            dir.deleteRecursively()
        }
    }
}
