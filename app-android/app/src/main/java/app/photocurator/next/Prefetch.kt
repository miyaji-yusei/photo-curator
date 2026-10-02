package app.photocurator.next

import android.content.Context
import android.util.Log
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import coil.compose.AsyncImage
import coil.request.ImageRequest
import coil.request.SuccessResult

/**
 * 選別の先読み（U26）。**次に出す写真を、出す前に読んでおく。**
 *
 * 先読みの量は控えめにする。NAS・Amazon では 1 枚ごとに原本や縮小の取得が
 * 走るので、多く・並べて頼むと、いま見ている写真の読み込みを遅くする。
 *   * スライドショーは次の 3 枚、トーナメントは次の 1 組（最大 6 枚）。
 *   * **同時には 1 枚だけ**（順に読む）。
 *   * 画面を離れる・次の写真へ進んで対象が変われば、読みかけは止める。
 */
object Prefetch {
    private const val TAG = "Prefetch"

    /** スライドショーで先に読む枚数。 */
    const val SLIDESHOW_AHEAD = 3

    /** トーナメントで先に読む組の数と、その上限枚数。 */
    const val TOURNAMENT_GROUPS = 1
    const val TOURNAMENT_MAX = 6

    /** 表示側と**同じ大きさ**で頼む（違うと Coil のメモリキャッシュが効かない）。 */
    const val SLIDESHOW_PX = 1600
    const val TILE_PX = 1280

    /**
     * 先読みする代表の相対パス。**core の queue の先頭が次の組**
     * （advance は先頭から groupSize 枚を取る）。連写は代表だけが並んでいる。
     */
    fun targets(queue: List<String>, groupSize: Int, slideshow: Boolean): List<String> =
        if (slideshow) {
            queue.take(SLIDESHOW_AHEAD)
        } else {
            queue.take(maxOf(1, groupSize) * TOURNAMENT_GROUPS).take(TOURNAMENT_MAX)
        }

    /**
     * 端末に置いてあるサムネイルがあるときだけ、その絵を返す。
     * **網へは行かない**（無いサムネイルを取りに行くと、表示用画像と同じ原本を
     * 二重に読みにいく）。端末の写真は原本が手元にあるので使わない。
     */
    fun cachedThumb(context: Context, photo: Photo): Any? = when (val r = photo.remote) {
        is SmbRef -> if (ThumbCache.has(context, r.nasId, r.path)) photo.thumbModel else null
        is AmazonRef ->
            if (ThumbCache.has(context, Amazon.linkOf(r.shareKey).cacheId, r.nodeId)) photo.thumbModel else null
        null -> null
    }
}

/**
 * 次の写真を **順に 1 枚ずつ** 読んでキャッシュに載せる。画面を離れる、または
 * 対象が変わると止まる。**表示しない。**
 */
@Composable
internal fun PrefetchAhead(photos: List<Photo>, edge: Int, px: Int) {
    val context = LocalContext.current
    val keys = photos.map { it.relativePath }
    LaunchedEffect(keys, edge, px) {
        val loader = Images.loader(context)
        for (photo in photos) {
            val started = System.currentTimeMillis()
            val request = ImageRequest.Builder(context)
                .data(photo.displayModel(edge))
                .size(px)
                .build()
            // execute は読み終わるまで待つ。次の 1 枚はそのあと。
            val result = loader.execute(request)
            if (result is SuccessResult) {
                Log.d("Prefetch", "先読み ${photo.name} ${result.dataSource} ${System.currentTimeMillis() - started}ms")
            }
        }
    }
}

/**
 * 選別で見せる 1 枚。**表示用画像が読めるまで、置いてあるサムネイルを先に出す。**
 * サムネイルが無ければ（端末の写真など）今までどおり。
 *
 * @param onSize 絵の大きさが分かったとき（サムネイルでも呼ぶ。表示用画像で上書きされる）。
 */
@Composable
internal fun DisplayImage(
    photo: Photo,
    displayEdge: Int,
    px: Int,
    modifier: Modifier = Modifier,
    onSize: ((Float, Float) -> Unit)? = null
) {
    val context = LocalContext.current
    var shown by remember(photo.relativePath, displayEdge) { mutableStateOf(false) }
    // U49: RAW の中にプレビューが無かった。**空白のままにせず、理由を出す**（選別・星は続けられる）。
    var rawFailed by remember(photo.relativePath, displayEdge) { mutableStateOf(false) }
    val thumb = remember(photo.relativePath) { Prefetch.cachedThumb(context, photo) }
    val loader = Images.loader(context)
    Box(modifier) {
        if (thumb != null && !shown) {
            AsyncImage(
                model = ImageRequest.Builder(context).data(thumb).size(480).build(),
                contentDescription = null,
                imageLoader = loader,
                contentScale = ContentScale.Fit,
                onSuccess = { state ->
                    val s = state.painter.intrinsicSize
                    if (!shown && s.width > 0f && s.height > 0f) onSize?.invoke(s.width, s.height)
                },
                modifier = Modifier.fillMaxSize()
            )
        }
        AsyncImage(
            model = ImageRequest.Builder(context)
                .data(photo.displayModel(displayEdge))
                .size(px)
                .build(),
            contentDescription = photo.name,
            imageLoader = loader,
            contentScale = ContentScale.Fit,
            onSuccess = { state ->
                shown = true
                Log.d("Prefetch", "表示 ${photo.name} ${state.result.dataSource}")
                val s = state.painter.intrinsicSize
                if (s.width > 0f && s.height > 0f) onSize?.invoke(s.width, s.height)
            },
            onError = { if (photo.isRaw) rawFailed = true },
            modifier = Modifier.fillMaxSize()
        )
        if (rawFailed && !shown) EmptyTile(Preview.Failed, raw = true)
    }
}
