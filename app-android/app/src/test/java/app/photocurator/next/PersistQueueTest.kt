package app.photocurator.next

import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test
import java.util.concurrent.atomic.AtomicInteger
import kotlin.coroutines.CoroutineContext

/**
 * U50（レビュー D14）: 選別画面の保存を、**画面の scope から起動しない**。
 * 確定の直後（同じフレーム）に戻ると、画面の scope が先に取り消され、まだ動いていない
 * `launch { Store.save(...) }` が走らないまま消える。保存は呼んだその場で
 * アプリの列（[Persist]）に積み、離れるときの同期は [Persist.settle] で書き終わりを待つ。
 */
class PersistQueueTest {
    /** Compose の画面の dispatcher の代わり。**頼まれても次のフレームまで動かさない。** */
    private class NextFrame : CoroutineDispatcher() {
        val waiting = ArrayList<Runnable>()
        override fun dispatch(context: CoroutineContext, block: Runnable) {
            waiting += block
        }
    }

    @Test fun 画面のscopeから起動した保存は戻ると消えうる() = runBlocking {
        // 前の書き方（Cull.kt の scope.launch { Store.save(...) }）で何が起きるかを固定する。
        val frame = NextFrame()
        val screen = CoroutineScope(Job() + frame)
        val wrote = AtomicInteger(0)
        val key = "test-old:" + System.nanoTime()
        screen.launch { Persist.latest(key) { wrote.incrementAndGet() } }
        screen.cancel() // 同じフレームで戻った
        frame.waiting.toList().forEach { it.run() }
        Persist.settle(key)
        assertEquals("前の書き方では最後の 1 組が書かれない", 0, wrote.get())
    }

    @Test fun その場で列に積めば戻っても書かれる() = runBlocking {
        val frame = NextFrame()
        val screen = CoroutineScope(Job() + frame)
        val wrote = AtomicInteger(0)
        val key = "test-new:" + System.nanoTime()
        Persist.submit(key) { wrote.incrementAndGet() }
        screen.cancel() // 同じフレームで戻った
        // 離れるときの同期（Sidecar の flush）は settle で待つ。積んだ分を書き終えてから返る。
        Persist.settle(key)
        assertEquals(1, wrote.get())
    }

    @Test fun 二つの書き込み先に順に積めば同期の前に両方書き終わる() = runBlocking {
        // applyOverrides は手直しと Session を続けて積む。flush はどちらも settle する。
        val order = java.util.Collections.synchronizedList(ArrayList<String>())
        val stamp = System.nanoTime()
        Persist.submit("overrides:t$stamp") { Thread.sleep(30); order += "overrides" }
        Persist.submit("session:t$stamp") { order += "session" }
        Persist.settle("session:t$stamp")
        Persist.settle("overrides:t$stamp")
        assertEquals(setOf("overrides", "session"), order.toSet())
    }

    @Test fun 積んだ直後に積んだものが最後に勝つ() = runBlocking {
        val last = AtomicInteger(-1)
        val key = "test-last:" + System.nanoTime()
        for (value in 1..50) Persist.submit(key) { last.set(value) }
        Persist.settle(key)
        assertEquals(50, last.get())
        assertFalse(last.get() < 50)
    }
}
