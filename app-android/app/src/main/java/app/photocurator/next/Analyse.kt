@file:OptIn(kotlin.ExperimentalUnsignedTypes::class)

package app.photocurator.next

import android.content.Context
import android.graphics.Bitmap
import android.util.Log
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uniffi.photo_curator_core.dHashFromGray

/**
 * 連写のまとめに使う指紋（dHash）を作る。
 *
 * **デコードと縮小は Android に任せる。** `loadThumbnail` は OS が持っている
 * 縮小画像を返すので、**原本を読まない**（実測 1 枚 3.5ms）。
 * Tauri 版は原本 6.7MB を読んでいて、2,000 枚で 13.4GB になっていた。
 *
 * 値を作る規則は core（Rust）が持つ。PC・Web と同じ値でなければ、
 * 端末をまたいだときにまとまり方が変わってしまう。
 */
object Analyse {
    private const val TAG = "Analyse"

    /**
     * 1 枚ぶんの dHash。作れなければ null。
     *
     * **null と「値が 0」は違う。** 読めなかったものを 0 にすると、
     * 読めない写真どうしが同一に見えて誤ってまとまる。
     */
    fun hash(context: Context, photo: Photo): String? {
        val source = Photos.thumbnail(context, photo, edge = 64) ?: return null
        return try {
            hashOf(source)
        } catch (error: Exception) {
            Log.w(TAG, "dHash を作れなかった: ${photo.name}", error)
            null
        } finally {
            source.recycle()
        }
    }

    /** 絵から指紋を作る。**元の Bitmap は片付けない**（呼んだ側の持ち物）。 */
    fun hashOf(source: Bitmap): String? {
        // **縮小は core に任せる。** ここで createScaledBitmap を使うと、
        // 縮小率が大きいときに 2x2 しか読まず、同じ絵でも値が揃わない
        // （実測で距離 5）。輝度をそのまま渡し、升目の平均は core が取る。
        val width = source.width
        val height = source.height
        val pixels = IntArray(width * height)
        source.getPixels(pixels, 0, width, 0, 0, width, height)
        val gray = ByteArray(width * height) { at ->
            val pixel = pixels[at]
            val r = (pixel shr 16) and 0xFF
            val g = (pixel shr 8) and 0xFF
            val b = pixel and 0xFF
            // 目の感度に合わせた重み。緑が一番効く。
            (((r * 299) + (g * 587) + (b * 114)) / 1000).toByte()
        }
        return dHashFromGray(gray.toUByteArray().toList(), width.toUInt(), height.toUInt())
    }

    /**
     * 指紋そのものが効いているかを確かめる。**まとまらない理由を切り分ける。**
     *
     * 「まとまらない」は「写真が本当に違う」でも起きるし「指紋が壊れている」でも
     * 起きる。区別がつかないまま閾値をいじると、いつまでも直らない。
     *
     * 同じ絵を JPEG で作り直したものと比べる。中身は同じなので、
     * **距離が 0 に近ければ指紋は効いている**。32 前後なら値が乱数と変わらない。
     */
    fun selfCheck(context: Context, photo: Photo) {
        val source = Photos.thumbnail(context, photo, edge = 64) ?: run {
            Log.w(TAG, "自己確認: 縮小画像を読めなかった")
            return
        }
        try {
            val original = hashOf(source)
            val stream = java.io.ByteArrayOutputStream()
            source.compress(Bitmap.CompressFormat.JPEG, 70, stream)
            val bytes = stream.toByteArray()
            val copy = android.graphics.BitmapFactory.decodeByteArray(bytes, 0, bytes.size)
            val again = copy?.let { hashOf(it) }
            copy?.recycle()
            if (original == null || again == null) {
                Log.w(TAG, "自己確認: 指紋を作れなかった")
                return
            }
            Log.i(
                TAG,
                "自己確認: ${source.width}x${source.height} / $original vs $again / " +
                    "距離 ${uniffi.photo_curator_core.hashDistance(original, again)}" +
                    "（0 に近ければ指紋は効いている）"
            )
        } catch (error: Exception) {
            Log.w(TAG, "自己確認に失敗", error)
        } finally {
            source.recycle()
        }
    }

    /**
     * いまの作り方の版。**変えたら上げる。**
     *
     * 3: 縮小を core の升目平均に移した。2 までは Android の縮小器に任せていて、
     *    同じ絵でも値が 5 ビット動いた。**古い値は比べられないので作り直す。**
     */
    const val VERSION = 3

    /**
     * まとめて作る。**すでにある分は作り直さない。**
     *
     * 作り直すのは 2 つの場合だけ:
     * - 版が違う（作り方が変わった）
     * - 原本の大きさが変わった（写真が差し替わった）
     *
     * 開くたびに全部やり直すと、何が起きているのか誰にも分からなくなる。
     * 進み具合は「試した枚数」で返す。**失敗も進捗のうち。**
     * 成功だけ数えると、読めない写真がある限り終わらないように見える。
     */
    suspend fun fingerprints(
        context: Context,
        photos: List<Photo>,
        cached: Map<String, Fingerprint>,
        onProgress: (done: Int, total: Int) -> Unit,
        onPartial: suspend (Map<String, Fingerprint>) -> Unit = {}
    ): Map<String, Fingerprint> = withContext(Dispatchers.IO) {
        // **既に分かっている分から始める。** 途中で止まったときにここを空から
        // 始めていると、まだ見ていない写真の指紋まで消してしまう。
        val out = HashMap(cached)
        var madeSinceSave = 0
        photos.forEachIndexed { index, photo ->
            val known = cached[photo.relativePath]
            val usable = known != null && known.version == VERSION && known.size == photo.size
            if (!usable) {
                val made = hash(context, photo)
                if (made != null) {
                    out[photo.relativePath] = Fingerprint(VERSION, photo.size, made)
                } else {
                    // 作れなかったものは控えない。**次に開いたときにもう一度試す。**
                    // 古い（大きさの違う）値が残っていたら消す。合わない値は
                    // 持っているより無い方がよい。
                    out.remove(photo.relativePath)
                }
                madeSinceSave += 1
            }
            if (index % 10 == 9 || index == photos.lastIndex) {
                onProgress(index + 1, photos.size)
            }
            // **途中でやめても、作った分は残す。**
            // 919 枚のアルバムを 800 枚まで数えて戻ったときに全部やり直しでは、
            // 二度と最後までたどり着けない。
            if (madeSinceSave >= 200) {
                onPartial(HashMap(out))
                madeSinceSave = 0
            }
        }
        // 最後まで来たときだけ、アルバムに無くなったものを片付ける。
        // 途中で刈ると、まだ見ていない写真を「消えた」と誤解する。
        val living = photos.mapTo(HashSet()) { it.relativePath }
        out.keys.retainAll(living)
        out
    }
}

/**
 * 連写がまとまらなかったときに、**どちらの条件で落ちたのか**を残す。
 *
 * 「まとまらない」には理由が 3 つある（指紋が無い・時間が離れている・
 * 見た目が違う）。区別できないと、直しようがない推測が始まる。
 */
object Neighbours {
    private const val TAG = "Neighbours"

    fun log(refs: List<uniffi.photo_curator_core.PhotoRef>, threshold: uniffi.photo_curator_core.BurstThreshold) {
        if (refs.size < 2) return
        var bothHashed = 0
        var nearInTime = 0
        var nearInLook = 0
        var both = 0
        val nearDistances = ArrayList<Int>()
        for (at in 1 until refs.size) {
            val previous = refs[at - 1]
            val photo = refs[at]
            val leftHash = previous.dHash
            val rightHash = photo.dHash
            val leftAt = previous.capturedAt
            val rightAt = photo.capturedAt
            if (leftHash == null || rightHash == null || leftAt == null || rightAt == null) continue
            bothHashed += 1
            val gap = kotlin.math.abs(leftAt - rightAt)
            val distance = uniffi.photo_curator_core.hashDistance(leftHash, rightHash)
            val timeOk = gap <= threshold.windowMs
            val lookOk = distance <= threshold.distance
            if (timeOk) nearInTime += 1
            if (lookOk) nearInLook += 1
            if (timeOk && lookOk) both += 1
            if (at <= 15) Log.i(TAG, "隣 #$at  ${gap}ms  距離 $distance")
            if (timeOk) nearDistances += distance.toInt()
        }
        Log.i(
            TAG,
            "隣どうし ${refs.size - 1} 組 / 両方に指紋 $bothHashed / " +
                "時間が近い $nearInTime / 見た目が近い $nearInLook / 両方 $both"
        )
        // **閾値を勘で決めないための材料。** 時間が近いペアだけの距離の散らばり。
        // 連写ならここが小さい側に固まり、別の絵なら 32 前後に寄る。
        if (nearDistances.isNotEmpty()) {
            val sorted = nearDistances.sorted()
            val buckets = IntArray(7)
            for (value in sorted) buckets[minOf(6, value / 5)] += 1
            Log.i(
                TAG,
                "時間が近いペアの距離: 中央 ${sorted[sorted.size / 2]} / 最小 ${sorted.first()} / " +
                    "0-4:${buckets[0]} 5-9:${buckets[1]} 10-14:${buckets[2]} 15-19:${buckets[3]} " +
                    "20-24:${buckets[4]} 25-29:${buckets[5]} 30+:${buckets[6]}"
            )
        }
    }
}
