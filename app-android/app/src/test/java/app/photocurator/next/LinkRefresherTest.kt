package app.photocurator.next

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withContext
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.concurrent.atomic.AtomicInteger

/**
 * U43（Amazon）: アプリを開き直した直後は、控えた一覧の tempLink が古い。並べて取る
 * 8 本が同時に失敗したとき、一覧を取り直すのは 1 本だけで、**残りが新しいリンクを使えず
 * 失敗のまま返っていた**（404 だと「リンクが削除されています」になり、準備ごと止まる）。
 * 再試行で通るのは、そのときにはもう新しいリンクが手元にあるから。
 */
class LinkRefresherTest {
    private val share = "host|share"

    @Test fun 同時に失敗しても取り直しは1回で全員が新しいリンクを使える() = runBlocking {
        val relists = AtomicInteger(0)
        val refresher = LinkRefresher(relist = { _ ->
            relists.incrementAndGet()
            delay(50) // 一覧の取得には時間がかかる。その間に他の 7 本も失敗する。
            SmbResult.Ok((1..8).associate { "n$it" to "new$it" })
        })
        val got = withContext(Dispatchers.Default) {
            (1..8).map { i -> async { refresher.replace(share, "n$i", "old$i") } }.awaitAll()
        }
        assertEquals(1, relists.get())
        assertEquals((1..8).map { SmbResult.Ok("new$it") }, got)
        assertEquals("new3", refresher.current("n3"))
    }

    @Test fun 新しいリンクも使えなければ間を置くまで取り直さない() = runBlocking {
        var clock = 0L
        val relists = AtomicInteger(0)
        val refresher = LinkRefresher(
            relist = { relists.incrementAndGet(); SmbResult.Ok(mapOf("n1" to "new1")) },
            now = { clock }
        )
        clock = 100_000
        assertEquals(SmbResult.Ok("new1"), refresher.replace(share, "n1", "old1"))
        // new1 も使えなかった。すぐには一覧を叩き直さない（1 分に 1 回まで）。
        clock += 1_000
        assertEquals(SmbResult.Ok(null), refresher.replace(share, "n1", "new1"))
        assertEquals(1, relists.get())
        clock += 61_000
        refresher.replace(share, "n1", "new1")
        assertEquals(2, relists.get())
    }

    @Test fun 一覧も取れなければその理由を返す() = runBlocking {
        val refresher = LinkRefresher(relist = { SmbResult.Failed(Amazon.GONE) })
        val got = refresher.replace(share, "n1", "old1")
        assertTrue(got is SmbResult.Failed)
        assertEquals(Amazon.GONE, (got as SmbResult.Failed).reason)
    }
}
