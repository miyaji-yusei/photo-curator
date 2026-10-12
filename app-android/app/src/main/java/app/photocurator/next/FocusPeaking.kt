package app.photocurator.next

import android.content.Context
import android.graphics.Bitmap
import android.graphics.drawable.BitmapDrawable
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.FilterQuality
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import coil.request.ImageRequest
import coil.request.SuccessResult
import coil.size.Scale
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import uniffi.photo_curator_core.focusMask

/** マスクの元にした画像。**表示用画像から出したものと原本から出したものは別。** */
enum class FocusKind { Display, Original }

/** 写真 1 枚・1 種類ぶんの鍵。id は道筋のハッシュなので、道筋も足して取り違えを避ける。 */
data class MaskKey(val id: Long, val path: String, val kind: FocusKind) {
    constructor(photo: Photo, kind: FocusKind) : this(photo.id, photo.relativePath, kind)
}

/**
 * core が返した強さ（0〜255）。**そのままアルファにして Lime で塗る。**
 * 色の付いた画素は見せるときに作る（持っておくのは強さだけで、1 枚 256KB 以下）。
 */
class FocusAlpha(val width: Int, val height: Int, val alpha: ByteArray) {
    /** 非プレマルチの ARGB。`Bitmap.createBitmap(IntArray, …)` にそのまま渡せる。 */
    fun toArgb(rgb: Int = FocusPeaking.COLOR_RGB): IntArray {
        val out = IntArray(width * height)
        val count = minOf(out.size, alpha.size)
        for (i in 0 until count) {
            out[i] = ((alpha[i].toInt() and 0xFF) shl 24) or (rgb and 0xFFFFFF)
        }
        return out
    }

    fun toBitmap(): Bitmap =
        Bitmap.createBitmap(toArgb(), width, height, Bitmap.Config.ARGB_8888)
}

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
 * マスクを **1 枚ずつ**（同時に 2 枚は計算しない）作り、写真ごとに覚える。
 * 失敗（読めない・空のマスク）は null を返し、覚えない。
 */
class MaskStore(limit: Int = 60) {
    private val cache = MaskCache<MaskKey, FocusAlpha>(limit)
    private val mutex = Mutex()

    fun cached(key: MaskKey): FocusAlpha? = cache.get(key)

    suspend fun load(key: MaskKey, produce: suspend () -> FocusAlpha?): FocusAlpha? {
        cache.get(key)?.let { return it }
        return mutex.withLock {
            // 待っているあいだに別の呼び出しが作っていたら、それを使う。
            cache.get(key) ?: produce()?.also { cache.put(key, it) }
        }
    }
}

/**
 * 「ピントが合っている所を見る」（U60）。**どこがピントかは core の `focusMask` が決める。**
 * ここは入力の縮め方・輝度への変換・色付け・覚え方・オン／オフの状態だけ。
 */
object FocusPeaking {
    /** 重ねる色。アプリの Lime（0xFFD6FF73）の RGB。 */
    const val COLOR_RGB = 0xD6FF73

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

    fun cached(photo: Photo, kind: FocusKind): FocusAlpha? = store.cached(MaskKey(photo, kind))

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

    /** core のマスクを [FocusAlpha] に。空（0 x 0）や長さ違いは null。 */
    fun alphaOf(width: UInt, height: UInt, data: List<UByte>): FocusAlpha? {
        val w = width.toInt()
        val h = height.toInt()
        if (w <= 0 || h <= 0 || data.size != w * h) return null
        val bytes = ByteArray(data.size)
        for (i in data.indices) bytes[i] = data[i].toByte()
        return FocusAlpha(w, h, bytes)
    }

    /**
     * 読めた画像からマスクを作る。**Default で動かす**（UI を止めない）。
     * 画像は読み出せる形（ハードウェアではない）であること。
     */
    suspend fun maskFor(photo: Photo, kind: FocusKind, bitmap: Bitmap): FocusAlpha? =
        store.load(MaskKey(photo, kind)) {
            withContext(Dispatchers.Default) { compute(bitmap, kind) }
        }

    /**
     * 表示用画像を **Coil から** 読んでマスクにする（選別画面用）。
     * 画面が使っているのと同じ読み込み器なので、NAS の写真も端末に置いた
     * 表示用画像（ディスクキャッシュ）から読む。**印のために原本は取りに行かない。**
     */
    suspend fun maskFromDisplay(context: Context, photo: Photo, displayEdge: Int): FocusAlpha? {
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

    private fun compute(source: Bitmap, kind: FocusKind): FocusAlpha? {
        if (source.isRecycled || source.config == Bitmap.Config.HARDWARE) return null
        val side = if (kind == FocusKind.Display) INPUT_DISPLAY else INPUT_ORIGINAL
        val (w, h) = fitWithin(source.width, source.height, side)
        if (w == 0) return null
        val scaled = if (w == source.width && h == source.height) source
        else Bitmap.createScaledBitmap(source, w, h, true)
        return try {
            val pixels = IntArray(w * h)
            scaled.getPixels(pixels, 0, w, 0, 0, w, h)
            val mask = focusMask(luminance(pixels), w.toUInt(), h.toUInt())
            alphaOf(mask.width, mask.height, mask.data)
        } finally {
            if (scaled !== source) scaled.recycle()
        }
    }
}

/**
 * マスクを写真の上に重ねる。**写真と同じ ContentScale.Fit** なので、同じ大きさの枠に
 * 置けば輪郭が写真に合う。拡大・移動は呼び出し側の [modifier]（graphicsLayer）で写真と揃える。
 * 押しても反応しない（ポインター入力を受けない）。
 */
@Composable
fun FocusOverlay(alpha: FocusAlpha, modifier: Modifier = Modifier) {
    val bitmap = remember(alpha) { alpha.toBitmap().asImageBitmap() }
    Image(
        bitmap = bitmap,
        contentDescription = null,
        contentScale = ContentScale.Fit,
        filterQuality = FilterQuality.Medium,
        modifier = modifier.fillMaxSize()
    )
}

/**
 * 選別画面のタイルに重ねる。オンのときだけ呼ぶ。表示用画像を読んで計算し、
 * 出来たら重なる。**写真が替わったら前の写真のマスクはすぐ外す**（id をキーにする）。
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
    shown?.let { FocusOverlay(it, modifier) }
}
