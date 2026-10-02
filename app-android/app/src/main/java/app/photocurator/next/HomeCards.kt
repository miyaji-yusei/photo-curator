package app.photocurator.next

import android.content.Context
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.withContext
import java.io.File
import java.util.concurrent.ConcurrentHashMap

/**
 * 全員で 1 回だけ計算を共有する。**誰も使わなくても、終わるのを待たない。**
 *
 * 以前は `async(start = LAZY)` を `coroutineScope` の中に置いたまま、誰も `await` しないと、
 * 始まっていない子を親が待ち続けて**ホームのカードが読み込み中のまま固まった**（実機で発覚）。
 * 使い終わったら共有の計算を必ず取り消す。
 */
internal suspend fun <T, R> withSharedLazy(
    compute: suspend () -> T,
    use: suspend CoroutineScope.(suspend () -> T) -> R
): R = coroutineScope {
    val shared = async(start = CoroutineStart.LAZY) { compute() }
    try {
        use(this) { shared.await() }
    } finally {
        shared.cancel()
    }
}

/**
 * 「署名が同じなら前の答えを返す」記憶（メモリの中だけ）。
 * ホームに戻るたびに全プロジェクトを読み直さないために使う（A3）。
 */
class Memo<K : Any, S, V> {
    private val entries = ConcurrentHashMap<K, Pair<S, V>>()

    /** [signature] が前回と同じなら記憶を返し、違えば [compute] して覚える。 */
    suspend fun get(key: K, signature: S, compute: suspend () -> V): V {
        val hit = entries[key]
        if (hit != null && hit.first == signature) return hit.second
        val value = compute()
        entries[key] = signature to value
        return value
    }

    fun forget(key: K) {
        entries.remove(key)
    }
}

/**
 * ホームのカード 1 枚ぶん（状態と見本の絵）。**並列に作り、変わっていなければ作り直さない。**
 *
 * 以前はプロジェクトごとに直列で、顔ぶれを 2 回（状態と見本）・ハッシュ値を全件パース・
 * renders を全列挙していた。いまは (1) プロジェクトを並列に、(2) 顔ぶれは 1 回だけ読み、
 * (3) 関係するファイルの更新時刻と大きさが前回と同じなら前の結果を使う。
 */
object HomeCards {
    data class Card(val standing: Standing, val cover: Any?)

    private val memo = Memo<String, List<Long>, Card>()

    /** ファイルの「更新時刻と大きさ」。無ければ -1。**中身を読まずに変わったかを見る。** */
    fun stamp(file: File): List<Long> =
        if (file.exists()) listOf(file.lastModified(), file.length()) else listOf(-1L, -1L)

    suspend fun load(context: Context, projects: List<Project>): Map<String, Card> =
        withSharedLazy({ Renders.tally(context) }) { tally ->
            projects.map { project ->
                async(Dispatchers.IO) { project.id to cardOf(context, project, tally) }
            }.awaitAll().toMap()
        }

    private suspend fun cardOf(
        context: Context,
        project: Project,
        tally: suspend () -> Map<Pair<String, Int>, Int>
    ): Card {
        val key = project.source.key
        val edge = Prefs.projectEdge(context, project.id)
        // 状態を決める材料のファイルと設定。**どれかが変われば作り直す。**
        val signature = buildList {
            addAll(stamp(Store.file(context, project.id)))
            addAll(stamp(Trouble.file(context, key)))
            addAll(stamp(Listing.file(context, key)))
            addAll(stamp(Fingerprints.file(context, key)))
            // 絵の置き場は、中身が増減するとフォルダの更新時刻が動く。
            addAll(stamp(Renders.dirOf(context)))
            addAll(stamp(ThumbCache.dirOf(context)))
            add(edge.toLong())
        }
        return memo.get(project.id, signature) {
            val known = Listing.load(context, key)
            Card(
                standing = standingOf(context, project, known, edge, tally),
                cover = Covers.coverOf(context, known)
            )
        }
    }
}
