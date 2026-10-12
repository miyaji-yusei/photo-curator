@file:OptIn(kotlin.ExperimentalUnsignedTypes::class)

package app.photocurator.next

import android.content.Context
import android.graphics.Bitmap
import android.util.Log
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.withContext
import uniffi.photo_curator_core.dHashFromGray

/**
 * 連写のまとめに使うハッシュ値（dHash）を作る。
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
     * 1 枚ぶんのハッシュ値の結果。**「作れた」「一時的に作れなかった」「非対応」を分ける。**
     *
     * 分け方（迷ったら一時的。非対応にすると、原本が変わるまで二度と試さず、選別の対象からも外れる）:
     * - 一時的: ファイルを開けない・読めない（許可・NAS の切断・Wi-Fi）、取得の失敗。次に開いたときにもう一度試す
     * - 非対応: **全部読めたのに復号できない**（壊れた画像・この端末で扱えない形式）、
     *   RAW で埋め込みのプレビューが無い。原本が変わらない限り、何度やっても同じ
     */
    sealed interface HashResult {
        data class Made(val print: Fingerprint) : HashResult
        data class Transient(val reason: String) : HashResult
        data class Unsupported(val reason: String) : HashResult
    }

    /**
     * 1 枚ぶんの dHash。
     *
     * **作れなかったことと「値が 0」は違う。** 読めなかったものを 0 にすると、
     * 読めない写真どうしが同一に見えて誤ってまとまる。
     * 絵は読めたのにハッシュ値だけ作れなかったとき（小さすぎるなど）は、**失敗にせず**
     * 空のハッシュ値で「作れなかった」の印を控える（選別には出る。連写のまとめに入らないだけ）。
     */
    fun hash(context: Context, photo: Photo): HashResult {
        // U49: RAW は OS が縮小画像を作れないことがある。**そのときは中のプレビューから作る。**
        val source = Photos.thumbnail(context, photo, edge = 64)
            ?: (if (photo.isRaw) RawImages.localThumbBitmap(context, photo.uri, 64) else null)
            ?: return localFailure(context, photo)
        return try {
            HashResult.Made(Fingerprint(VERSION, photo.size, hashOf(source) ?: ""))
        } catch (error: Exception) {
            Log.w(TAG, "dHash を作れなかった: ${photo.name}", error)
            HashResult.Transient("ハッシュ値を作れませんでした")
        } finally {
            source.recycle()
        }
    }

    /**
     * 端末の写真の縮小画像が取れなかったとき、**開けなかったのか・読めたのに復号できないのか**を分ける。
     * 開けない（許可・SD の取り外しなど）は一時的。開けて、画像として読めない／RAW にプレビューが無い
     * なら非対応。縮小だけ失敗して、画像としては読めるなら一時的（安全側）。
     */
    private fun localFailure(context: Context, photo: Photo): HashResult = try {
        val opened = context.contentResolver.openInputStream(photo.uri)?.use { it.read(); true } ?: false
        when {
            !opened -> HashResult.Transient("ファイルを開けませんでした（移動・削除・権限）")
            photo.isRaw -> HashResult.Unsupported("RAW に埋め込みのプレビューがありません")
            else -> {
                val bounds = android.graphics.BitmapFactory.Options().apply { inJustDecodeBounds = true }
                context.contentResolver.openInputStream(photo.uri)?.use {
                    android.graphics.BitmapFactory.decodeStream(it, null, bounds)
                }
                if (bounds.outWidth > 0) HashResult.Transient("縮小画像を作れませんでした")
                else HashResult.Unsupported("画像を読み取れませんでした（破損または非対応の形式）")
            }
        }
    } catch (error: Exception) {
        Log.w(TAG, "読めない理由を調べられなかった: ${photo.name}", error)
        HashResult.Transient("ファイルを開けませんでした")
    }

    /**
     * NAS の 1 枚。**先頭 64KB だけ読んで、ハッシュ値と撮影時刻を同時に取る。**
     *
     * 原本 6MB を網越しに引くと 2,000 枚で 12GB になる。EXIF は先頭にあり、
     * その中の縮小画像（160x120 程度）でハッシュ値は十分に作れる。
     * 読めなかったら一時的（次に開いたときにもう一度）。
     */
    fun hashOverNetwork(
        context: Context,
        reader: Smb.Reader,
        photo: Photo,
        fallbackAt: Long
    ): HashResult {
        val path = photo.smb?.path ?: return HashResult.Transient("NAS の道筋が分かりません")
        if (photo.isRaw) return hashRawOverNetwork(context, reader, photo, path, fallbackAt)
        val head = reader.head(path, SmbExifReader.HEAD_BYTES)
            ?: return HashResult.Transient("NAS から読めませんでした")
        val exif = SmbExifReader.parse(head, fallbackAt)
        // 縮小画像が無い写真はハッシュ値を作らない。**原本を引きに行かない。**
        // 連写のまとめに入らないだけで、選別には出る。
        val bytes = exif.thumbnail
            ?: return HashResult.Made(Fingerprint(VERSION, photo.size, "", exif.takenAt))
        return try {
            val decoded = android.graphics.BitmapFactory.decodeByteArray(bytes, 0, bytes.size)
                ?: return HashResult.Made(Fingerprint(VERSION, photo.size, "", exif.takenAt))
            // **向きを当ててからハッシュ値を作る。** 回ったままだと、同じ連写でも
            // 縦横が混ざって距離が開き、まとまらなくなる。
            val bitmap = SmbExifReader.applyOrientation(decoded, exif.orientation)
            // **一度読んだ絵は捨てない。** 一覧を出すたびに網へ行かせない。
            photo.smb?.let { ThumbCache.write(context, it.nasId, it.path, bitmap) }
            val made = hashOf(bitmap)
            bitmap.recycle()
            HashResult.Made(Fingerprint(VERSION, photo.size, made ?: "", exif.takenAt))
        } catch (error: Exception) {
            Log.w(TAG, "NAS のハッシュ値を作れなかった: ${photo.name}", error)
            HashResult.Made(Fingerprint(VERSION, photo.size, "", exif.takenAt))
        }
    }

    /**
     * NAS の RAW（U49）。**IFD をたどって、小さいプレビューと撮影時刻だけを読む。**
     * 25MB の原本は引かない。
     *
     * - 読み取りが失敗した（接続・I/O）なら一時的
     * - **最後まで読めたのにプレビューが無い・復号できない**なら非対応（表示用画像も作れないので）
     */
    private fun hashRawOverNetwork(
        context: Context,
        reader: Smb.Reader,
        photo: Photo,
        path: String,
        fallbackAt: Long
    ): HashResult {
        // ranged は、読み取りで例外が出ても「プレビューが無い」ときも null を返し、さらに
        // 抽出の中（RawPreview.inspect）が読み取りの例外を飲み込む。**そのまま null を非対応にすると、
        // NAS の瞬断が「非対応」に化けて二度と試されない。** 読み取りの例外は自分で見張って覚える。
        var ioFailed = false
        var ran = false
        val got = reader.ranged(path) { source ->
            val watched = ByteSource { offset, length ->
                try {
                    source.read(offset, length)
                } catch (error: Exception) {
                    ioFailed = true
                    throw error
                }
            }
            RawImages.extract(watched, thumb = true).also { ran = true }
        }
        if (got == null) {
            return if (ran && !ioFailed) HashResult.Unsupported("RAW に埋め込みのプレビューがありません")
            else HashResult.Transient("NAS から読めませんでした")
        }
        val takenAt = got.takenAt ?: fallbackAt
        return try {
            val bitmap = RawImages.decode(got.jpeg, got.orientation, 256)
                ?: return HashResult.Unsupported("RAW のプレビューを復号できませんでした")
            val small = RawImages.shrink(bitmap, RawImages.THUMB_STORE)
            if (small !== bitmap) bitmap.recycle()
            photo.smb?.let { ThumbCache.write(context, it.nasId, it.path, small) }
            val made = hashOf(small)
            small.recycle()
            HashResult.Made(Fingerprint(VERSION, photo.size, made ?: "", takenAt))
        } catch (error: Exception) {
            Log.w(TAG, "NAS の RAW のハッシュ値を作れなかった: ${photo.name}", error)
            HashResult.Transient("RAW のプレビューを読めませんでした")
        }
    }

    /**
     * Amazon の写真のハッシュ値。**縮小して返してもらったサムネイルから作る。**
     * 撮影時刻は一覧に入っていたものをそのまま控える（EXIF を読まない）。
     *
     * リンクが消えていたら**準備ごと止める**（つまずきとして出すため）。
     * それ以外で取れなければ一時的（次に開いたときにもう一度試す）。
     * 取れたのに復号できないときは**失敗にしない**（空のハッシュ値。Amazon が作った縮小画像で、
     * 選別には出る。非対応にも一時的にもすると、終わらない準備になる。U58 の仮置きとの違い）。
     */
    suspend fun hashOverAmazon(context: Context, photo: Photo): HashResult {
        val ref = photo.amazon ?: return HashResult.Transient("Amazon の指し先が分かりません")
        val cacheId = Amazon.linkOf(ref.shareKey).cacheId
        val bytes = ThumbCache.read(context, cacheId, ref.nodeId) ?: run {
            when (val got = Amazon.image(ref, Amazon.THUMB)) {
                is SmbResult.Failed -> {
                    if (got.reason == Amazon.GONE) throw IllegalStateException(Amazon.GONE)
                    return HashResult.Transient("Amazon から取得できませんでした")
                }
                is SmbResult.Ok -> got.value.also { ThumbCache.put(context, cacheId, ref.nodeId, it) }
            }
        }
        val bitmap = android.graphics.BitmapFactory.decodeByteArray(bytes, 0, bytes.size)
            ?: return HashResult.Made(Fingerprint(VERSION, photo.size, "", photo.takenAt))
        val made = hashOf(bitmap)
        bitmap.recycle()
        return HashResult.Made(Fingerprint(VERSION, photo.size, made ?: "", photo.takenAt))
    }

    /** 絵からハッシュ値を作る。**元の Bitmap は片付けない**（呼んだ側の持ち物）。 */
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
     * ハッシュ値そのものが効いているかを確かめる。**まとまらない理由を切り分ける。**
     *
     * 「まとまらない」は「写真が本当に違う」でも起きるし「ハッシュ値が壊れている」でも
     * 起きる。区別がつかないまま閾値をいじると、いつまでも直らない。
     *
     * 同じ絵を JPEG で作り直したものと比べる。中身は同じなので、
     * **距離が 0 に近ければハッシュ値は効いている**。32 前後なら値が乱数と変わらない。
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
                Log.w(TAG, "自己確認: ハッシュ値を作れなかった")
                return
            }
            Log.i(
                TAG,
                "自己確認: ${source.width}x${source.height} / $original vs $again / " +
                    "距離 ${uniffi.photo_curator_core.hashDistance(original, again)}" +
                    "（0 に近ければハッシュ値は効いている）"
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

    /** 控えたハッシュ値が、いまの作り方・いまの原本のものか。 */
    fun upToDate(known: Fingerprint?, photo: Photo): Boolean =
        known != null && known.version == VERSION && known.size == photo.size

    /**
     * 全部の写真が**片付いているか**（いまのハッシュ値がある。または**非対応と確定していて原本が
     * 変わっていない**。非対応はもう試さないので、待っても増えない）。
     */
    fun allUpToDate(
        photos: List<Photo>,
        prints: Map<String, Fingerprint>,
        failures: Map<String, Failure> = emptyMap()
    ): Boolean = photos.all { settled(it, prints, failures) }

    /** この写真はもう試さなくてよいか（ハッシュ値が最新、または非対応で原本が同じ）。 */
    fun settled(photo: Photo, prints: Map<String, Fingerprint>, failures: Map<String, Failure>): Boolean =
        upToDate(prints[photo.relativePath], photo) || Failures.isUnsupported(photo, failures[photo.relativePath])

    /** [fingerprints] の結果。ハッシュ値と、試して駄目だったものの控え。 */
    data class Prints(val prints: Map<String, Fingerprint>, val failures: Map<String, Failure>)

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
     *
     * **非対応と確定した写真（原本が同じ）は試さず、分母にも入れない。**
     * 一時的に作れなかった写真は控えに理由を残し、次に開いたときにもう一度試す。
     */
    suspend fun fingerprints(
        context: Context,
        photos: List<Photo>,
        cached: Map<String, Fingerprint>,
        onProgress: (done: Int, total: Int) -> Unit,
        onPartial: suspend (Prints) -> Unit = {},
        // NAS のときだけ要る。**端末の写真には触らせない。**
        nasAccess: Pair<Nas, String>? = null,
        // 試して駄目だったものの控え。
        known: Map<String, Failure> = emptyMap(),
        // 端末の写真を 1 枚読む（試験で差し替える）。
        local: (Photo) -> HashResult = { hash(context, it) }
    ): Prints = withContext(Dispatchers.IO) {
        val sweep = Sweep(photos, cached, known, onProgress, onPartial)

        /**
         * NAS はまとめて並べて読む。**待ち時間が支配的**なので、
         * 何本か同時に投げるだけで大きく変わる。増やしすぎると NAS 側が
         * 詰まるので、少なめに抑える。
         */
        suspend fun sweepNetwork(reader: Smb.Reader) = coroutineScope {
            for (chunk in sweep.active.chunked(8)) {
                val results = chunk.map { photo ->
                    async {
                        photo to if (sweep.needsWork(photo)) {
                            hashOverNetwork(context, reader, photo, photo.takenAt)
                        } else null
                    }
                }.map { it.await() }
                for ((photo, made) in results) sweep.record(photo, made)
            }
        }

        /**
         * Amazon は縮小を向こうに頼む。**サムネを取って、そのままハッシュ値にする。**
         * 取ったサムネは一覧でも使うので置いておく（二度取らない）。
         */
        suspend fun sweepAmazon() = coroutineScope {
            for (chunk in sweep.active.chunked(Amazon.PARALLEL)) {
                val results = chunk.map { photo ->
                    async {
                        photo to if (sweep.needsWork(photo)) hashOverAmazon(context, photo) else null
                    }
                }.map { it.await() }
                for ((photo, made) in results) sweep.record(photo, made)
            }
        }

        if (nasAccess != null && sweep.active.any(sweep::needsWork)) {
            // **1 本の接続で全部読む。** 1 枚ごとに張り直すと、網の往復が
            // そのまま待ち時間になる（実測 50 枚で 40 秒）。
            val (nas, password) = nasAccess
            val result = Smb.reading(nas, password) { reader -> sweepNetwork(reader) }
            if (result is SmbResult.Failed) {
                // **失敗を握りつぶさない。** ここまでに作った分は残してから、準備を失敗にする。
                onPartial(sweep.snapshot())
                throw IllegalStateException(result.reason)
            }
        } else if (photos.any { it.amazon != null }) {
            sweepAmazon()
        } else {
            sweepLocal(sweep, local)
        }

        sweep.finish()
    }

    /** 端末の写真を 1 枚ずつ読む。**非対応と確定した写真は `active` に入らないので読まない。** */
    internal suspend fun sweepLocal(sweep: Sweep, read: (Photo) -> HashResult) {
        for (photo in sweep.active) {
            sweep.record(photo, if (sweep.needsWork(photo)) read(photo) else null)
        }
    }

    /**
     * [fingerprints] の 1 回ぶんの状態。**結果の書き込みと進みの数え方だけを持つ**
     * （読む処理は持たない。試験でそのまま動かせる）。
     */
    internal class Sweep(
        private val photos: List<Photo>,
        private val cached: Map<String, Fingerprint>,
        private val known: Map<String, Failure>,
        private val onProgress: (done: Int, total: Int) -> Unit,
        private val onPartial: suspend (Prints) -> Unit,
        private val now: () -> Long = System::currentTimeMillis
    ) {
        // **既に分かっている分から始める。** 途中で止まったときにここを空から
        // 始めていると、まだ見ていない写真のハッシュ値まで消してしまう。
        private val out = HashMap(cached)
        private val failures = HashMap(known)

        /**
         * 今回見る写真。**非対応と確定していて原本が同じものは外す**（読まない・分母に入れない）。
         * 外した写真の控えはそのまま残る。
         */
        val active: List<Photo> = photos.filter {
            upToDate(cached[it.relativePath], it) || !Failures.isUnsupported(it, known[it.relativePath])
        }

        private var total = active.size
        private var done = 0

        // **新しく作った（変わった）分だけを数える。** 準備済みの写真を通るだけで
        // 50 回ごとにファイルを書き直さない（A4）。
        private val unsaved = PartialCounter(50)

        fun needsWork(photo: Photo): Boolean = !upToDate(cached[photo.relativePath], photo)

        fun snapshot() = Prints(HashMap(out), HashMap(failures))

        /** [result] が null なら、読む必要が無かった（準備済み）。 */
        suspend fun record(photo: Photo, result: HashResult?) {
            val path = photo.relativePath
            when (result) {
                null -> Unit
                is HashResult.Made -> {
                    out[path] = result.print
                    failures.remove(path)
                }
                // 作れなかったものはハッシュ値を控えない。**次に開いたときにもう一度試す。**
                // 古い（大きさの違う）値が残っていたら消す。理由は一覧に出すために控える。
                is HashResult.Transient -> {
                    out.remove(path)
                    failures[path] = Failure(FailureKind.Transient, result.reason, photo.size, VERSION, now())
                }
                is HashResult.Unsupported -> {
                    out.remove(path)
                    failures[path] = Failure(FailureKind.Unsupported, result.reason, photo.size, VERSION, now())
                    // **分母から外す。** 選別に出さない写真を「あと何枚」に数えない。
                    total -= 1
                }
            }
            if (result !is HashResult.Unsupported) done += 1
            if (done % 10 == 0 || done == total || result is HashResult.Unsupported) onProgress(done, total)
            // **途中でやめても、作った分は残す。**
            if (unsaved.add(result != null)) onPartial(snapshot())
        }

        /**
         * 最後まで来たときだけ、無くなったものを片付ける。
         * 途中で刈ると、まだ見ていない写真を「消えた」と誤解する。
         * **顔ぶれが空なら刈らない。** 取れなかっただけかもしれず、全部消えてしまう。
         */
        fun finish(): Prints {
            if (photos.isNotEmpty()) {
                val living = photos.mapTo(HashSet()) { it.relativePath }
                out.keys.retainAll(living)
                failures.keys.retainAll(living)
            }
            return Prints(out, failures)
        }
    }
}

object Prepare {
    /**
     * 準備（ハッシュ値・サムネイル・表示用画像）を進める順。**選別と同じ、撮影時刻の昇順
     * （同時刻は相対パス）。** 先の写真から使えるようになるので、選別が始まる順に
     * 用意する。NAS の一覧は名前順、Amazon は日付の昇順（同時刻の並びは不定）で
     * 来るので、そのまま使わずここで並べる。並びだけで、中身は変えない。
     */
    fun inShootingOrder(photos: List<Photo>): List<Photo> =
        photos.sortedWith(compareBy({ it.takenAt }, { it.relativePath }))

    /** 新しく取った顔ぶれをどうするか。 */
    enum class ListingDecision {
        /** 控えを置き換える。 */
        Replace,

        /** 控えは書かない（元から無く、今回も空）。 */
        Skip,

        /** 前は写真があったのに空で返ってきた。**前の控えを残し、失敗として扱う。** */
        KeepPrevious,

        /**
         * 一覧が欠けている（読めなかったフォルダがある・上限で止めた。U50・D8）。
         * **控えもハッシュ値も触らず（前の控えを残し）、失敗として扱う。**
         */
        Incomplete
    }

    /**
     * 取り直した顔ぶれを控えに書いてよいか。**空で上書きしない。欠けた一覧で上書きしない。**
     * 失敗が空・欠けに見える形（網の途切れなど）で、写真とハッシュ値を失うのを防ぐ。
     *
     * 前の控えが無くても、欠けた一覧は控えにしない。控えると次からそれが「前の一覧」になり、
     * 欠けたまま選別が始まってしまう（端末だけでなく NAS の相手にも、写真が無いように見える）。
     */
    fun decideListing(previous: List<Photo>?, fresh: List<Photo>, complete: Boolean = true): ListingDecision = when {
        !complete -> ListingDecision.Incomplete
        fresh.isNotEmpty() -> ListingDecision.Replace
        !previous.isNullOrEmpty() -> ListingDecision.KeepPrevious
        else -> ListingDecision.Skip
    }

    /** 欠けた一覧を使わなかったときの言い方（「止まっています」に出る）。 */
    fun incompleteReason(listed: Listed, hadPrevious: Boolean): String {
        val what = if (listed.incomplete > 0) "一部のフォルダを読めませんでした"
        else "写真が多すぎるか、フォルダが深すぎて、全部を数えられませんでした"
        return what + if (hadPrevious) "。前の一覧のままです" else "。読めるようにしてから再試行してください"
    }

    /**
     * 取り直した一覧を控えに書き、使う一覧を返す。**欠けていれば書かずに理由を投げる**
     * （このあとのハッシュ値の刈り込みまで進ませない）。[save] は控えへの書き込み。
     */
    suspend fun settleListing(
        previous: List<Photo>?,
        listed: Listed,
        save: suspend (List<Photo>) -> Unit
    ): List<Photo> {
        val fresh = listed.photos
        when (decideListing(previous, fresh, listed.complete)) {
            ListingDecision.Replace -> save(fresh)
            ListingDecision.Skip -> Unit
            ListingDecision.KeepPrevious ->
                throw IllegalStateException("写真の一覧を取れませんでした（前の状態は残してあります）")
            ListingDecision.Incomplete ->
                throw IllegalStateException(incompleteReason(listed, hadPrevious = !previous.isNullOrEmpty()))
        }
        return fresh
    }

    /**
     * ハッシュ値を作り直す写真があるか。**無ければ NAS へつながない**（U43）。
     * 非対応と確定した写真（原本が同じ）は数えない（もう試さないので、つなぐ理由にならない）。
     */
    fun needsNetwork(
        photos: List<Photo>,
        cached: Map<String, Fingerprint>,
        failures: Map<String, Failure> = emptyMap()
    ): Boolean = photos.any { !Analyse.settled(it, cached, failures) }

    suspend fun run(
        context: android.content.Context,
        project: Project,
        // **true のときだけ数え直す。** 押されたときだけ網へ行く。
        rescan: Boolean = false,
        onProgress: (done: Int, total: Int) -> Unit
    ): Pair<List<Photo>, List<uniffi.photo_curator_core.PhotoRef>> {
        // 顔ぶれは控えたものを使う。開くたびに数え直すと、NAS では
        // そのたびに網の往復が要る。
        val previous = Listing.load(context, project.source.key)
        val listed = if (!rescan && previous != null) previous else {
            // U49: 組の RAW を外すかはプロジェクトの設定。変えたら「写真を再読み込み」で反映。
            // U50: 欠けた一覧（読めなかったフォルダ・上限）なら、控えもハッシュ値も触らずに止まる。
            val fresh = Photos.listing(context, project.source, Prefs.pairRawJpeg(context, project.id))
            settleListing(previous, fresh) { Listing.save(context, project.source.key, it) }
        }
        val photos = inShootingOrder(listed)
        val cached = Fingerprints.load(context, project.source.key)
        val known = Failures.load(context, project.source.key)

        // NAS のときだけ、つなぎ先とパスワードを渡す。**作るものが無ければつながない**（U43）。
        // 準備済みのプロジェクトを開くたびに NAS へつないでいると、Wi-Fi・NAS が起きる前に
        // 開いただけで「止まっています」になる（A1 で接続の失敗を出すようにしたため）。
        val nasAccess = if (project.source.kind == SourceKind.Nas && needsNetwork(photos, cached, known)) {
            val nasId = project.source.key.substringBefore("|")
            NasStore.all(context).firstOrNull { it.id == nasId }?.let { nas ->
                NasPasswords.password(context, nas)?.let { nas to it }
            } ?: throw IllegalStateException("NAS につなぐ情報（登録かパスワード）がありません")
        } else null

        val made = Analyse.fingerprints(
            context, photos, cached,
            onProgress = onProgress,
            onPartial = { saveBoth(context, project.source.key, it, cached, known) },
            nasAccess = nasAccess,
            known = known
        )
        saveBoth(context, project.source.key, made, cached, known)

        return assemble(context, project, photos, made.prints, made.failures)
    }

    /** 変わったほうだけ書く（ハッシュ値と、解析できなかったものの控え）。 */
    private suspend fun saveBoth(
        context: android.content.Context,
        key: String,
        made: Analyse.Prints,
        cached: Map<String, Fingerprint>,
        known: Map<String, Failure>
    ) {
        if (made.prints != cached) Fingerprints.save(context, key, made.prints)
        if (made.failures != known) Failures.save(context, key, made.failures)
    }

    /**
     * 選別・学習が使う入口。**準備を二重に走らせない**（A5）。
     *
     * 1. 控えだけで組めるなら、それを使う（準備済みなら一瞬。網へ行かず、何も書かない）。
     * 2. 詳細画面の準備が走っているあいだは、その分が控えに出てくるのを待つ
     *    （同じ NAS へ 2 本つなぎ、同じファイルに書くのを避ける）。
     * 3. そうでなければ、これまでどおり [run] で足りない分だけ作る。
     *
     * **ハッシュ値が足りないまま始めることはしない**（連写のまとまりが変わるため）。
     */
    suspend fun ready(
        context: android.content.Context,
        project: Project,
        onProgress: (done: Int, total: Int) -> Unit
    ): Pair<List<Photo>, List<uniffi.photo_curator_core.PhotoRef>> {
        loadReady(context, project)?.let { return it }
        while (Preparations.of(project.id).running) {
            val progress = Preparations.of(project.id).meta
            onProgress(progress.first, progress.second)
            kotlinx.coroutines.delay(500)
            loadReady(context, project)?.let { return it }
        }
        return run(context, project, false, onProgress)
    }

    /**
     * 控えだけで組めるなら、組んで返す（**網へ行かない・何も書き直さない**）。
     * ハッシュ値が全部そろっていなければ null（作る側 [run] へ）。
     * 選別・学習が、準備済みなのに準備を自前でもう一度回さないために使う（A5）。
     */
    suspend fun loadReady(
        context: android.content.Context,
        project: Project
    ): Pair<List<Photo>, List<uniffi.photo_curator_core.PhotoRef>>? {
        val listing = Listing.load(context, project.source.key)
        if (listing.isNullOrEmpty()) return null
        val prints = Fingerprints.load(context, project.source.key)
        val failures = Failures.load(context, project.source.key)
        val photos = inShootingOrder(listing)
        if (!Analyse.allUpToDate(photos, prints, failures)) return null
        return assemble(context, project, photos, prints, failures)
    }

    private suspend fun assemble(
        context: android.content.Context,
        project: Project,
        photos: List<Photo>,
        prints: Map<String, Fingerprint>,
        failures: Map<String, Failure> = emptyMap()
    ): Pair<List<Photo>, List<uniffi.photo_curator_core.PhotoRef>> {
        // **撮影時刻は EXIF のものを使う。**
        // NAS の更新時刻はコピーしたときに変わるので、撮影順にならない。
        // ハッシュ値と同じ読みで取れているので、ここで差し替えて並べ直す。
        val dated = photos
            .map { photo ->
                // Amazon は一覧の時刻が正（EXIF を読んでいない）。ハッシュ値に控えた古い値で
                // 上書きすると、読み方を直しても直らない。
                val takenAt = if (photo.amazon != null) null else prints[photo.relativePath]?.takenAt
                if (takenAt != null && takenAt > 0) photo.copy(takenAt = takenAt) else photo
            }
            .let(::inShootingOrder)

        // 撮影時刻を当てたものを控え直す。**次に開いたときはここから始まる。**
        if (dated != photos) Listing.save(context, project.source.key, dated)

        // **選別に渡す refs だけ、非対応を外す。** 返す写真の一覧（対応表）には残す
        // （途中のセッションの写真・★を引けるように。サイドカーの顔ぶれも変えない）。
        val refs = selectable(dated, failures).map {
            uniffi.photo_curator_core.PhotoRef(
                relativePath = it.relativePath,
                capturedAt = it.takenAt,
                // **作れなかったものは null のまま。** 0 を入れると
                // 読めない写真どうしが同一に見えて誤ってまとまる。
                // 空文字も「作れなかった」の印として扱う。
                dHash = prints[it.relativePath]?.hash?.takeIf { hash -> hash.isNotEmpty() },
                dHashVersion = Analyse.VERSION
            )
        }
        return dated to refs
    }

    /** 選別の対象にする写真。**非対応と確定していて原本が同じものを除く。** */
    fun selectable(photos: List<Photo>, failures: Map<String, Failure>): List<Photo> =
        if (failures.isEmpty()) photos
        else photos.filterNot { Failures.isUnsupported(it, failures[it.relativePath]) }

    /**
     * 表示用画像を作る。**準備の 3 段目。原本を読むのはここだけ。**
     *
     * 1 枚 6MB を網越しに読むので、ここがいちばん時間がかかる。だから
     * **できた分から選別に出せる**ようにしてあり、途中で止めても残る。
     * 端末のアルバムには作らない（手元のファイルは Coil が直接デコードする）。
     *
     * 戻り値は「作った枚数」。すでにあるものは数え直さない。
     */
    suspend fun renders(
        context: android.content.Context,
        project: Project,
        photos: List<Photo>,
        edge: Int,
        onProgress: (done: Int, total: Int) -> Unit
    ): Int = withContext(Dispatchers.IO) {
        if (project.source.kind == SourceKind.Amazon) {
            return@withContext rendersAmazon(context, project, photos, edge, onProgress)
        }
        val nasId = project.source.key.substringBefore("|")
        val nas = NasStore.all(context).firstOrNull { it.id == nasId }
            ?: return@withContext 0
        val password = NasPasswords.password(context, nas) ?: return@withContext 0

        val missing = Prepare.inShootingOrder(photos).filter { photo ->
            val path = photo.smb?.path ?: return@filter false
            when {
                Renders.has(context, nasId, path, edge) -> false
                // **大きい絵から縮めて作れるなら、原本を読み直さない。**
                Renders.deriveFromLarger(context, nasId, path, edge) -> false
                else -> true
            }
        }
        var done = photos.size - missing.size
        onProgress(done, photos.size)
        if (missing.isEmpty()) return@withContext 0

        var made = 0
        val result = Smb.reading(nas, password) { reader ->
            // **1 本の接続で通す。** 原本は大きいので、並べすぎると
            // 端末のメモリと NAS の両方を圧迫する。少しずつ重ねる。
            for (chunk in missing.chunked(3)) {
                coroutineScope {
                    chunk.map { photo ->
                        async {
                            val path = photo.smb?.path ?: return@async false
                            // U49: RAW は**中の大きいプレビューだけ**を読む（原本 25MB は引かない）。
                            if (photo.isRaw) {
                                val got = reader.ranged(path) { RawImages.extract(it, thumb = false) }
                                    ?: return@async false
                                return@async Renders.write(context, nasId, path, edge, got.jpeg, got.orientation)
                            }
                            val whole = reader.whole(path) ?: return@async false
                            val orientation = SmbExifReader.parse(whole, 0L).orientation
                            Renders.write(context, nasId, path, edge, whole, orientation)
                        }
                    }.map { it.await() }
                }.forEach { if (it) made += 1 }
                done += chunk.size
                onProgress(done, photos.size)
            }
        }
        // **接続や認証の失敗を成功にしない。** 準備の側の「止まっています」に載せる。
        if (result is SmbResult.Failed) throw IllegalStateException(result.reason)
        made
    }

    /**
     * Amazon の表示用画像。**縮小は向こうに頼む。原本は読まない。**
     *
     * 最初に 1 枚だけ上限を測り（設計 08 章 8.5）、選んでいた大きさが出せなければ
     * 出せる中で一番大きいものに直す。リンクが消えていたら準備ごと止める。
     */
    private suspend fun rendersAmazon(
        context: android.content.Context,
        project: Project,
        photos: List<Photo>,
        edge: Int,
        onProgress: (done: Int, total: Int) -> Unit
    ): Int {
        val link = Amazon.linkOf(project.source.key)
        val refs = Prepare.inShootingOrder(photos).mapNotNull { it.amazon }
        if (refs.isEmpty()) return 0
        if (Prefs.amazonMaxEdge(context, link.shareId) == 0) {
            Amazon.measureMaxEdge(refs.first())?.let { Prefs.setAmazonMaxEdge(context, link.shareId, it) }
        }
        val usable = Prefs.usableEdge(edge, Prefs.amazonMaxEdge(context, link.shareId))
        if (usable != edge) Prefs.setProjectEdge(context, project.id, usable)

        val missing = refs.filter { ref ->
            when {
                Renders.has(context, link.cacheId, ref.nodeId, usable) -> false
                // **大きい絵がもうあるなら、それを縮めるだけ。** 1024 で作ったあとに
                // 768 へ落とすとき、Amazon から取り直さない。
                Renders.deriveFromLarger(context, link.cacheId, ref.nodeId, usable) -> false
                else -> true
            }
        }
        var done = refs.size - missing.size
        onProgress(done, refs.size)
        var made = 0
        for (chunk in missing.chunked(Amazon.PARALLEL)) {
            coroutineScope {
                chunk.map { ref ->
                    async {
                        when (val got = Amazon.image(ref, usable)) {
                            is SmbResult.Ok -> Renders.put(context, link.cacheId, ref.nodeId, usable, got.value)
                            is SmbResult.Failed -> {
                                if (got.reason == Amazon.GONE) throw IllegalStateException(Amazon.GONE)
                                false
                            }
                        }
                    }
                }.map { it.await() }
            }.forEach { if (it) made += 1 }
            done += chunk.size
            onProgress(done, refs.size)
        }
        return made
    }

    /**
     * EXIF に縮小画像が無かった写真を、**表示用画像から**埋める。
     *
     * 書き出し方によっては EXIF に縮小画像が入らない（実機の NAS にあった
     * 「倉坂くるる」の 17 枚がそうで、ハッシュ値が全部空だった）。すると
     *
     *   * 連写がまとまらない（ハッシュ値が無いので比べようがない）
     *   * カバーが出ない（端末に小さい絵が 1 枚も無い）
     *
     * の 2 つが、何も言わずに起きる。表示用画像は準備で**もう落としてある**
     * ので、そこから作れば網へは行かない。
     *
     * ハッシュ値の元が EXIF の縮小画像か表示用画像かで、同じ写真でも値は少しずれる。
     * まとまりの判定は距離で見ているので、そこは吸収できる範囲に収まる。
     */
    suspend fun fillFromRenders(
        context: android.content.Context,
        project: Project,
        photos: List<Photo>,
        edge: Int
    ): Int = withContext(Dispatchers.IO) {
        val key = project.source.key
        val nasId = key.substringBefore("|")
        val prints = Fingerprints.load(context, key)
        val filled = HashMap<String, Fingerprint>(prints)
        var made = 0
        for (photo in Prepare.inShootingOrder(photos)) {
            val path = photo.smb?.path ?: continue
            val print = prints[photo.relativePath] ?: continue
            if (print.hash.isNotEmpty()) continue
            val file = Renders.file(context, nasId, path, edge)
            if (!file.exists() || file.length() == 0L) continue
            try {
                // **ハッシュ値に要るのは形だけ。** 大きいまま読むと 17 枚でも重い。
                val options = android.graphics.BitmapFactory.Options().apply { inSampleSize = 4 }
                val bitmap = android.graphics.BitmapFactory.decodeFile(file.path, options)
                    ?: continue
                filled[photo.relativePath] = print.copy(hash = Analyse.hashOf(bitmap) ?: "")
                // カバーにも使う。**表示用画像を毎回開かせない。**
                ThumbCache.write(context, nasId, path, bitmap)
                bitmap.recycle()
                made += 1
            } catch (error: Exception) {
                Log.w("Prepare", "表示用画像から作れなかった: ${photo.name}", error)
            }
        }
        if (made > 0) Fingerprints.save(context, key, filled)
        made
    }
}

/**
 * 連写がまとまらなかったときに、**どちらの条件で落ちたのか**を残す。
 *
 * 「まとまらない」には理由が 3 つある（ハッシュ値が無い・時間が離れている・
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
            "隣どうし ${refs.size - 1} 組 / 両方にハッシュ値 $bothHashed / " +
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
