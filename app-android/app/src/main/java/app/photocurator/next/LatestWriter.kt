package app.photocurator.next

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch

/**
 * **同じ書き込み先への保存を 1 本に並べ、最後に頼まれたものが必ず勝つ**ようにする。
 *
 * 選別は確定のたびに保存する。画面の scope で毎回 `launch` すると、前の保存が終わる前に
 * 次が始まり、(1) 同じ一時ファイルに同時に書いて混ざる、(2) 先に始まった古い保存が後から
 * 置き換わって新しい状態を戻す、が起きる。
 *
 * ここは**アプリの寿命の scope** で 1 本だけ書く。書いている間に頼まれたものは、
 * 最新の 1 つだけ残して詰める（途中の状態は書かなくてよい。最後が残ればよい）。
 * [submit] が返す待ちは、**その値（かそれより新しい値）が書き終わったら**完了する。
 */
class LatestWriter<T>(
    private val scope: CoroutineScope,
    private val write: suspend (T) -> Unit,
    private val onError: (Exception) -> Unit = {}
) {
    private class Box<T>(val value: T)

    private val lock = Any()
    private var pending: Box<T>? = null
    private var waiters = ArrayList<CompletableDeferred<Unit>>()
    private var running: Job? = null

    fun submit(value: T): CompletableDeferred<Unit> {
        val done = CompletableDeferred<Unit>()
        synchronized(lock) {
            pending = Box(value)
            waiters.add(done)
            if (running == null) running = scope.launch { drain() }
        }
        return done
    }

    private suspend fun drain() {
        while (true) {
            var next: Box<T>? = null
            var batch: ArrayList<CompletableDeferred<Unit>> = ArrayList()
            synchronized(lock) {
                next = pending
                if (next == null) {
                    running = null
                } else {
                    pending = null
                    batch = waiters
                    waiters = ArrayList()
                }
            }
            val box = next ?: return
            try {
                write(box.value)
            } catch (error: kotlinx.coroutines.CancellationException) {
                batch.forEach { it.complete(Unit) }
                throw error
            } catch (error: Exception) {
                onError(error)
            }
            batch.forEach { it.complete(Unit) }
        }
    }
}

/**
 * アプリの寿命の保存の列。**書き込み先（key）ごとに 1 本**で、最後に頼まれたものが勝つ。
 *
 * 画面の scope ではなくアプリの scope で書くので、画面を離れても書きかけにならない。
 * [job] の中で失敗を扱う（失敗しても次の保存は止まらない）。
 */
object Persist {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val writers = java.util.concurrent.ConcurrentHashMap<String, LatestWriter<() -> Unit>>()

    suspend fun latest(key: String, job: () -> Unit) {
        writers.computeIfAbsent(key) {
            LatestWriter(
                scope,
                write = { it() },
                onError = { android.util.Log.w("Persist", "保存に失敗した: $key", it) }
            )
        }.submit(job).await()
    }
}
