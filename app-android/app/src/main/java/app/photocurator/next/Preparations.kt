package app.photocurator.next

import android.content.Context
import android.util.Log
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

/**
 * 準備の持ち主。**画面ではなくアプリが持つ。**
 *
 * これまでは詳細画面の中で走らせていたので、**戻った瞬間に止まっていた**。
 * 別のプロジェクトを選別しているあいだに、こちらの準備が進まない。
 *
 * ここに移すと、アプリを開いているあいだは進み続ける。バックグラウンドまでは
 * やらない（電池と通信の扱いを別に設計する必要があるため。→ 設計書 P2）。
 *
 * **NAS への往復は 1 本に並べる。** 2 つのプロジェクトが同時に網へ行くと、
 * どちらも遅くなって、しかも「どちらが進んでいるのか」が読めなくなる。
 */
object Preparations {
    private const val TAG = "Preparations"

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    /** 網へ行く仕事は 1 本ずつ。**端末の写真は関係ないので待たせない。** */
    private val network = Mutex()

    /** Amazon の仕事も 1 本に並べる。**NAS とは別の列**（相手が違うので待たせない）。 */
    private val amazonLine = Mutex()

    private val jobs = HashMap<String, Job>()

    /** プロジェクトごとの進み。（撮影時刻・サムネイル, 表示用画像）。 */
    data class Progress(
        val meta: Pair<Int, Int> = 0 to 0,
        val display: Pair<Int, Int> = 0 to 0,
        val running: Boolean = false,
        val trouble: String? = null
    )

    private val progress = MutableStateFlow<Map<String, Progress>>(emptyMap())

    fun watch(): StateFlow<Map<String, Progress>> = progress

    fun of(projectId: String): Progress = progress.value[projectId] ?: Progress()

    private fun put(projectId: String, change: (Progress) -> Progress) {
        progress.value = progress.value.toMutableMap().apply {
            put(projectId, change(this[projectId] ?: Progress()))
        }
    }

    /**
     * 準備を始める（もう走っているなら何もしない）。
     *
     * **押されたときだけ数え直す**という決まりは変えない。`rescan` はここでも
     * 「写真を再読み込み」からしか true にならない。
     */
    fun ensure(
        context: Context,
        project: Project,
        rescan: Boolean = false,
        displayEdge: Int,
        onFinished: () -> Unit = {}
    ) {
        val running = jobs[project.id]
        if (running != null && running.isActive && !rescan) return
        running?.cancel()

        val app = context.applicationContext
        jobs[project.id] = scope.launch {
            put(project.id) { it.copy(running = true, trouble = null) }
            try {
                val kind = project.source.kind
                val work: suspend () -> Unit = {
                    val ready = Prepare.run(app, project, rescan) { done, total ->
                        put(project.id) { it.copy(meta = done to total) }
                    }
                    if (project.source.remote) {
                        Prepare.renders(app, project, ready.first, displayEdge) { done, total ->
                            put(project.id) { it.copy(display = done to total) }
                        }
                        // EXIF に縮小画像が無かった写真を、落とした表示用画像から
                        // 埋める。**網へは行かない。**
                        // Amazon は縮小画像から指紋を作るので要らない。
                        if (kind == "nas") Prepare.fillFromRenders(app, project, ready.first, displayEdge)
                    }
                    Trouble.clear(app, project.source.key)
                }
                // **網へ行く仕事だけ 1 本に並べる。** 相手ごとに別の列。
                when (kind) {
                    "nas" -> network.withLock { work() }
                    "amazon" -> amazonLine.withLock { work() }
                    else -> work()
                }
                put(project.id) { it.copy(running = false) }
                onFinished()
            } catch (error: kotlinx.coroutines.CancellationException) {
                throw error
            } catch (error: Exception) {
                // **黙って落とさない。** 画面が無くても記録は残す。
                val said = Smb.describe(error)
                Log.w(TAG, "準備が途中で止まった: ${project.name}", error)
                Trouble.note(app, project.source.key, said)
                put(project.id) { it.copy(running = false, trouble = said) }
            }
        }
    }
}
