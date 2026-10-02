package app.photocurator.next

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay

/**
 * 準備の失敗を、**少し待って自動でやり直す**（U43）。
 *
 * プロジェクトを開いた瞬間は、端末の Wi-Fi がつなぎ直している途中だったり、NAS が
 * 眠りから起きる途中だったりする。そこで 1 回つまずいただけで「止まっています」にすると、
 * 数秒後に「再試行」を押せば通るのに、人が押すまで止まって見える。
 *
 * 開いたときも「再試行」も、準備はこの入口を通る（[Preparations.ensure]）。
 * **最後まで失敗したときだけ**理由を投げる（そこで初めて「止まっています」と再試行が出る）。
 */
object Retrying {
    /** やり直す前に待つ時間。**控えめに 3 回まで**（合わせて 17 秒）。 */
    val WAITS = listOf(2_000L, 5_000L, 10_000L)

    /**
     * 待っても直らない理由。**パスワード違いは繰り返すと NAS のアカウントが
     * 締め出されうる**ので、1 回で止める。登録・パスワードが無いのも人が入れるまで直らない。
     */
    private val PERMANENT = listOf(
        "パスワード", // ユーザー名かパスワードが違います／パスワードが要ります／登録かパスワード
        "NAS の登録が見つかりません",
        "共有名が見つかりません",
        "読む権限がありません",
        "共有フォルダではありません"
    )

    /** 人の言葉にした理由（[Smb.describe]）から、待てば直りうるかを決める。 */
    fun worthRetrying(said: String): Boolean = PERMANENT.none { said.contains(it) }

    suspend fun <T> run(
        waits: List<Long> = WAITS,
        worthRetrying: (Exception) -> Boolean,
        wait: suspend (Long) -> Unit = { delay(it) },
        onRetry: (attempt: Int, error: Exception) -> Unit = { _, _ -> },
        work: suspend () -> T
    ): T {
        var attempt = 0
        while (true) {
            try {
                return work()
            } catch (error: CancellationException) {
                // 画面を離れた・数え直しで取り消した。**失敗ではない**（A9）。
                throw error
            } catch (error: Exception) {
                if (attempt >= waits.size || !worthRetrying(error)) throw error
                onRetry(attempt + 1, error)
                wait(waits[attempt])
                attempt += 1
            }
        }
    }
}
