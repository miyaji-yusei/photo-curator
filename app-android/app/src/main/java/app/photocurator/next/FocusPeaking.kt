package app.photocurator.next

import android.content.Context
import android.graphics.Bitmap
import android.graphics.drawable.BitmapDrawable
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.ui.Alignment
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import coil.request.ImageRequest
import coil.request.SuccessResult
import coil.size.Scale
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import uniffi.photo_curator_core.FocusRegion
import uniffi.photo_curator_core.focusRegions

/** マスクの元にした画像。**表示用画像から出したものと原本から出したものは別。** */
enum class FocusKind { Display, Original }

/** 写真 1 枚・1 種類ぶんの鍵。id は道筋のハッシュなので、道筋も足して取り違えを避ける。 */
data class MaskKey(val id: Long, val path: String, val kind: FocusKind) {
    constructor(photo: Photo, kind: FocusKind) : this(photo.id, photo.relativePath, kind)
}

/**
 * core が返した、くっきりした所の四角（0〜1）と、解析した画像の大きさ（縦横比）。
 * [regions] が空なら「くっきりした所なし」（失敗ではない）。
 */
class FocusFound(val width: Int, val height: Int, val regions: List<FocusRegion>)

/** かぎ括弧の線 1 本（画面の px）。 */
data class BracketSegment(val x1: Float, val y1: Float, val x2: Float, val y2: Float)

/** 順位ごとの線の太さ（dp）と濃さ。 */
data class BracketStyle(val widthDp: Float, val alpha: Float)

/** 使った順に古いものから捨てる、上限つきのキャッシュ。 */
class MaskCache<K, V>(private val limit: Int) {
    private val entries = object : LinkedHashMap<K, V>(16, 0.75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<K, V>?): Boolean =
            size > maxOf(1, limit)
    }

    @Synchronized fun get(key: K): V? = entries[key]
    @Synchronized fun put(key: K, value: V) { entries[key] = value }
    @Synchronized fun clear() = entries.clear()
    val size: Int @Synchronized get() = entries.size
}

/**
 * 四角を **1 枚ずつ**（同時に 2 枚は計算しない）求め、写真ごとに覚える。
 * 失敗（読めない）は null を返し、覚えない。**四角が 0 個なのは失敗ではない**（覚える）。
 */
class MaskStore(limit: Int = 60) {
    private val cache = MaskCache<MaskKey, FocusFound>(limit)
    private val mutex = Mutex()

    fun cached(key: MaskKey): FocusFound? = cache.get(key)

    suspend fun load(key: MaskKey, produce: suspend () -> FocusFound?): FocusFound? {
        cache.get(key)?.let { return it }
        return mutex.withLock {
            // 待っているあいだに別の呼び出しが作っていたら、それを使う。
            cache.get(key) ?: produce()?.also { cache.put(key, it) }
        }
    }
}

/**
 * 「ピントが合っている所を見る」（U60）。**どこがくっきりしているかは core の `focusRegions` が決める。**
 * ここは入力の縮め方・輝度への変換・かぎ括弧の形・覚え方・オン／オフの状態だけ。
 * （名前は初期の「ピーキング」のまま。いまは輪郭ではなく領域の枠を出す。）
 */
object FocusPeaking {
    /** 「くっきりした所なし」の札。選別のタイルなど狭い所では短いほう。PC・Web と同じ文言。 */
    const val NONE_LABEL = "くっきりした所が見つかりません"
    const val NONE_LABEL_COMPACT = "ピント甘め？"

    /** かぎ括弧の腕の長さ（四角の短辺に対する割合）。 */
    private const val BRACKET_ARM = 0.3f

    /** いちばん強い 1 個（rank 0）は太く、2・3 個目は細く薄く。 */
    fun bracketStyle(rank: Int): BracketStyle =
        if (rank == 0) BracketStyle(3f, 1f) else BracketStyle(1.5f, 0.6f)

    /**
     * 写真を `Fit` で置いたときの範囲（left, top, width, height）。拡大はしない。
     * 読めない大きさは 0 の範囲。
     */
    fun fitRect(boxW: Float, boxH: Float, imageW: Int, imageH: Int): FloatArray {
        if (boxW <= 0f || boxH <= 0f || imageW <= 0 || imageH <= 0) return floatArrayOf(0f, 0f, 0f, 0f)
        val scale = minOf(boxW / imageW, boxH / imageH)
        val w = imageW * scale
        val h = imageH * scale
        return floatArrayOf((boxW - w) / 2f, (boxH - h) / 2f, w, h)
    }

    /**
     * 四角の**四隅だけ**のかぎ括弧（8 本の線）。[fit] は [fitRect] の結果。
     * 四角が小さくても腕が重ならないよう、腕は短辺の 30 %。
     */
    fun bracketSegments(region: FocusRegion, fit: FloatArray): List<BracketSegment> {
        val x0 = fit[0] + region.x * fit[2]
        val y0 = fit[1] + region.y * fit[3]
        val x1 = fit[0] + (region.x + region.w) * fit[2]
        val y1 = fit[1] + (region.y + region.h) * fit[3]
        val arm = minOf(x1 - x0, y1 - y0) * BRACKET_ARM
        return listOf(
            BracketSegment(x0, y0, x0 + arm, y0), BracketSegment(x0, y0, x0, y0 + arm),
            BracketSegment(x1, y0, x1 - arm, y0), BracketSegment(x1, y0, x1, y0 + arm),
            BracketSegment(x0, y1, x0 + arm, y1), BracketSegment(x0, y1, x0, y1 - arm),
            BracketSegment(x1, y1, x1 - arm, y1), BracketSegment(x1, y1, x1, y1 - arm)
        )
    }

    /** core に渡す輝度の長辺。表示用画像と原本とで分ける（PC・Web と同じ）。 */
    const val INPUT_DISPLAY = 1024
    const val INPUT_ORIGINAL = 1536

    /** 覚えておく写真の数。 */
    const val CACHE_LIMIT = 60

    /**
     * オン・オフ。**アプリを開いているあいだだけ**覚える（設定の同期には入れない）。
     * 選別画面と拡大画面で同じ状態を見る。
     */
    val enabled = mutableStateOf(false)

    fun toggle() {
        enabled.value = !enabled.value
    }

    private val store = MaskStore(CACHE_LIMIT)

    fun cached(photo: Photo, kind: FocusKind): FocusFound? = store.cached(MaskKey(photo, kind))

    /** 長辺が [maxSide] に収まる大きさ。拡大はしない。読めない大きさは 0 x 0。 */
    fun fitWithin(width: Int, height: Int, maxSide: Int): Pair<Int, Int> {
        if (width <= 0 || height <= 0 || maxSide <= 0) return 0 to 0
        val scale = minOf(1.0, maxSide.toDouble() / maxOf(width, height))
        return maxOf(1, Math.round(width * scale).toInt()) to
            maxOf(1, Math.round(height * scale).toInt())
    }

    /** ARGB の画素を BT.709 の輝度（0.2126R + 0.7152G + 0.0722B）にする。アルファは見ない。 */
    fun luminance(argb: IntArray): List<UByte> {
        val out = ArrayList<UByte>(argb.size)
        for (p in argb) {
            val r = (p shr 16) and 0xFF
            val g = (p shr 8) and 0xFF
            val b = p and 0xFF
            out.add(((2126 * r + 7152 * g + 722 * b + 5000) / 10000).toUByte())
        }
        return out
    }

    /**
     * core の結果を [FocusFound] に。大きさが読めなければ null、四角は 0〜1 に収まるものだけ。
     * **四角が 0 個でも null ではない**（「くっきりした所なし」）。
     */
    fun foundOf(width: Int, height: Int, regions: List<FocusRegion>): FocusFound? {
        if (width <= 0 || height <= 0) return null
        val valid = regions.filter {
            it.w > 0f && it.h > 0f && it.x >= 0f && it.y >= 0f &&
                it.x + it.w <= 1.001f && it.y + it.h <= 1.001f
        }
        return FocusFound(width, height, valid.take(MAX_REGIONS))
    }

    /** 四角の最大数（core と同じ）。 */
    const val MAX_REGIONS = 3

    /**
     * 読めた画像から四角を求める。**Default で動かす**（UI を止めない）。
     * 画像は読み出せる形（ハードウェアではない）であること。
     */
    suspend fun maskFor(photo: Photo, kind: FocusKind, bitmap: Bitmap): FocusFound? =
        store.load(MaskKey(photo, kind)) {
            withContext(Dispatchers.Default) { compute(bitmap, kind) }
        }

    /**
     * 表示用画像を **Coil から** 読んでマスクにする（選別画面用）。
     * 画面が使っているのと同じ読み込み器なので、NAS の写真も端末に置いた
     * 表示用画像（ディスクキャッシュ）から読む。**印のために原本は取りに行かない。**
     */
    suspend fun maskFromDisplay(context: Context, photo: Photo, displayEdge: Int): FocusFound? {
        val key = MaskKey(photo, FocusKind.Display)
        store.cached(key)?.let { return it }
        return store.load(key) {
            val request = ImageRequest.Builder(context)
                .data(photo.displayModel(displayEdge))
                .size(INPUT_DISPLAY)
                .scale(Scale.FIT)
                .allowHardware(false)
                .build()
            val result = Images.loader(context).execute(request) as? SuccessResult
            val bitmap = (result?.drawable as? BitmapDrawable)?.bitmap
            if (bitmap == null) null
            else withContext(Dispatchers.Default) { compute(bitmap, FocusKind.Display) }
        }
    }

    private fun compute(source: Bitmap, kind: FocusKind): FocusFound? {
        if (source.isRecycled || source.config == Bitmap.Config.HARDWARE) return null
        val side = if (kind == FocusKind.Display) INPUT_DISPLAY else INPUT_ORIGINAL
        val (w, h) = fitWithin(source.width, source.height, side)
        if (w == 0) return null
        val scaled = if (w == source.width && h == source.height) source
        else Bitmap.createScaledBitmap(source, w, h, true)
        return try {
            val pixels = IntArray(w * h)
            scaled.getPixels(pixels, 0, w, 0, 0, w, h)
            val found = focusRegions(luminance(pixels), w.toUInt(), h.toUInt())
            foundOf(w, h, found.regions)
        } finally {
            if (scaled !== source) scaled.recycle()
        }
    }
}

/**
 * くっきりした所の枠を写真の上に重ねる。カメラの AF 枠のように四角の**四隅だけ**のかぎ括弧。
 * 暗い縁取りを先に太く敷いてから Lime を重ねる（白い写真でも見える）。
 * 位置は**写真と同じ ContentScale.Fit** で置くので、同じ大きさの枠に置けば合う。
 * 拡大・移動は呼び出し側の [modifier]（graphicsLayer）で写真と揃える（枠だけが動き、札は動かない）。
 * 四角が 0 個のときは写真の下端に札を出す。押しても反応しない（ポインター入力を受けない）。
 */
@Composable
fun FocusOverlay(found: FocusFound, modifier: Modifier = Modifier, compact: Boolean = false) {
    val density = LocalDensity.current
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val boxW = with(density) { maxWidth.toPx() }
        val boxH = with(density) { maxHeight.toPx() }
        val fit = remember(boxW, boxH, found) { FocusPeaking.fitRect(boxW, boxH, found.width, found.height) }
        if (found.regions.isEmpty()) {
            val bottomGap = with(density) { (boxH - (fit[1] + fit[3])).toDp() } + 8.dp
            Box(Modifier.fillMaxSize().padding(bottom = bottomGap), contentAlignment = Alignment.BottomCenter) {
                Text(
                    if (compact) FocusPeaking.NONE_LABEL_COMPACT else FocusPeaking.NONE_LABEL,
                    color = Color.White,
                    fontSize = 11.sp,
                    maxLines = 1,
                    modifier = Modifier
                        .background(Color.Black.copy(alpha = 0.6f), RoundedCornerShape(50))
                        .padding(horizontal = 10.dp, vertical = 3.dp)
                )
            }
        } else {
            Canvas(modifier.fillMaxSize()) {
                // 弱い順に描く（強い 1 個が最後＝いちばん上）。
                found.regions.withIndex().reversed().forEach { (rank, region) ->
                    val style = FocusPeaking.bracketStyle(rank)
                    val segments = FocusPeaking.bracketSegments(region, fit)
                    for ((color, extra) in listOf(Color.Black.copy(alpha = 0.7f) to 2f, Lime to 0f)) {
                        for (s in segments) {
                            drawLine(
                                color = color.copy(alpha = color.alpha * style.alpha),
                                start = Offset(s.x1, s.y1),
                                end = Offset(s.x2, s.y2),
                                strokeWidth = (style.widthDp + extra).dp.toPx(),
                                cap = StrokeCap.Square
                            )
                        }
                    }
                }
            }
        }
    }
}

/**
 * 選別画面のタイルに重ねる。オンのときだけ呼ぶ。表示用画像を読んで計算し、
 * 出来たら重なる（計算中は何も出さない）。**写真が替わったら前の写真の結果はすぐ外す**（id をキーにする）。
 */
@Composable
fun FocusTileOverlay(photo: Photo, displayEdge: Int, modifier: Modifier = Modifier) {
    val context = LocalContext.current
    var shown by remember(photo.id, photo.relativePath) {
        mutableStateOf(FocusPeaking.cached(photo, FocusKind.Display))
    }
    LaunchedEffect(photo.id, photo.relativePath, displayEdge) {
        if (shown == null) shown = FocusPeaking.maskFromDisplay(context, photo, displayEdge)
    }
    shown?.let { FocusOverlay(it, modifier, compact = true) }
}
