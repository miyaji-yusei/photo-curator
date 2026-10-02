package app.photocurator.next

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

/**
 * U43: プロジェクトを開いたときの準備が、NAS・Wi-Fi が起きる前の一時的な失敗で
 * 「止まっています」にならないこと。開いたときと「再試行」は同じ入口（[Retrying.run]）を通る。
 */
class RetryingTest {
    /**
     * 偽の NAS。最初の `failures` 回はつながらず（[Smb.reading] が Failed を返す形）、
     * そのあとは一覧を返す。準備の側（Analyse）は Failed を理由付きの例外にして投げる。
     */
    private class FakeNas(private val failures: Int, private val reason: String = "応答がありません。同じ Wi-Fi につながっているか確認してください") {
        var calls = 0
        fun reading(): SmbResult<List<String>> {
            calls += 1
            return if (calls <= failures) SmbResult.Failed(reason) else SmbResult.Ok(listOf("a.jpg", "b.jpg"))
        }

        /** 準備 1 回分。Failed は投げる（Analyse.fingerprints と同じ）。 */
        fun prepare(): List<String> = when (val got = reading()) {
            is SmbResult.Failed -> throw IllegalStateException(got.reason)
            is SmbResult.Ok -> got.value
        }
    }

    private val waited = mutableListOf<Long>()

    private fun <T> open(nas: FakeNas, work: suspend () -> T): T = runBlocking {
        Retrying.run(
            worthRetrying = { Retrying.worthRetrying(it.message.orEmpty()) },
            wait = { waited += it },
            work = work
        )
    }

    @Test fun 開いたとき最初の2回つながらなくても自動で立ち直る() {
        val nas = FakeNas(failures = 2)
        val listed = open(nas) { nas.prepare() }
        assertEquals(listOf("a.jpg", "b.jpg"), listed)
        assertEquals(3, nas.calls)
        // 控えめに: 短い間隔から。
        assertEquals(Retrying.WAITS.take(2), waited)
    }

    @Test fun 再試行を押したときと同じ結果になる() {
        // 直す前の再現: 開いたとき 1 回で諦めると失敗し、少し後の「再試行」で通っていた。
        val nas = FakeNas(failures = 2)
        val opened = open(nas) { nas.prepare() }
        // 「再試行」は同じ入口をもう一度通るだけ。NAS はもう起きているので 1 回で通る。
        waited.clear()
        val retried = open(nas) { nas.prepare() }
        assertEquals(retried, opened)
        assertEquals(4, nas.calls)
        assertTrue(waited.isEmpty())
    }

    @Test fun 最後まで失敗したときだけ理由を投げる() {
        val nas = FakeNas(failures = 99)
        try {
            open(nas) { nas.prepare() }
            fail("失敗を成功にしてはいけない")
        } catch (error: IllegalStateException) {
            assertEquals("応答がありません。同じ Wi-Fi につながっているか確認してください", error.message)
        }
        assertEquals(Retrying.WAITS.size + 1, nas.calls)
        assertEquals(Retrying.WAITS, waited)
    }

    @Test fun パスワード違いはやり直さない() {
        // 何度も試すと NAS のアカウントが締め出されうる。直らないものは 1 回で止める。
        val nas = FakeNas(failures = 99, reason = "ユーザー名かパスワードが違います")
        try {
            open(nas) { nas.prepare() }
            fail()
        } catch (error: IllegalStateException) {
            assertEquals("ユーザー名かパスワードが違います", error.message)
        }
        assertEquals(1, nas.calls)
        assertTrue(waited.isEmpty())
    }

    @Test fun 取り消しはやり直さずそのまま投げる() {
        var calls = 0
        try {
            open(FakeNas(0)) { calls += 1; throw CancellationException("画面を離れた") }
            fail()
        } catch (error: CancellationException) {
            assertEquals(1, calls)
        }
        assertTrue(waited.isEmpty())
    }

    @Test fun 直らない理由の見分け() {
        assertFalse(Retrying.worthRetrying("ユーザー名かパスワードが違います"))
        assertFalse(Retrying.worthRetrying("NAS につなぐ情報（登録かパスワード）がありません"))
        assertFalse(Retrying.worthRetrying("NAS の登録が見つかりません"))
        assertFalse(Retrying.worthRetrying("NAS のパスワードが要ります"))
        assertFalse(Retrying.worthRetrying("共有名が見つかりません"))
        assertFalse(Retrying.worthRetrying("このユーザーには読む権限がありません"))
        assertTrue(Retrying.worthRetrying("ネットワークにつながっていません"))
        assertTrue(Retrying.worthRetrying("応答がありません。同じ Wi-Fi につながっているか確認してください"))
        assertTrue(Retrying.worthRetrying("接続を拒否されました。ホストとポートを確認してください"))
        assertTrue(Retrying.worthRetrying("写真の一覧を取れませんでした（前の状態は残してあります）"))
        assertTrue(Retrying.worthRetrying(Amazon.GONE))
    }
}
