package app.photocurator.next

import android.content.ContentUris
import android.content.Context
import android.graphics.Bitmap
import android.os.Build
import android.provider.MediaStore
import android.util.Log
import android.util.Size
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * 端末の写真を読む。**Tauri 版と違い、ここが最短距離。**
 *
 * 以前は Kotlin で読んだものを JSON にして JNI で Rust へ渡し、Rust から
 * WebView へ返していた。いまは Compose が直接この結果を使う。
 */

data class Album(
    val id: String,
    val name: String,
    val count: Int,
    /** 一覧の見出しに出す 1 枚。 */
    val coverId: Long,
    /**
     * 端末の中での置き場所（"Pictures/Camera/" のような形）。
     * **写真を移すときの行き先。** 表示名だけでは、同じ名前の別フォルダと
     * 区別できないし、どこへ移るのかを人に見せられない。
     */
    val relativeDir: String
)

data class Photo(
    val id: Long,
    val name: String,
    /** フォルダ内の相対パス。**共有するときの鍵。** */
    val relativePath: String,
    val size: Long,
    val takenAt: Long,
    /** NAS の写真ならその指し先。端末の写真なら null。 */
    val smb: SmbRef? = null,
    /** Amazon の共有リンクの写真ならその指し先（設計 08 章）。 */
    val amazon: AmazonRef? = null
) {
    /** この端末での指し先。共有しない。 */
    val uri get() = ContentUris.withAppendedId(COLLECTION, id)

    /**
     * 小さく並べるときの絵。**EXIF の縮小画像（160x120）。**
     * 詳細の一覧と、まとまりの確認だけ。選別には使わない。
     */
    val thumbModel: Any
        get() = smb?.let { SmbImage(it.nasId, it.path, SmbSize.Thumb) }
            ?: amazon?.let { AmazonImage(it, SmbSize.Thumb) } ?: uri

    /**
     * **選別と連写判定で見る絵。** 表示用画像（長辺 1024/1536）。
     *
     * 端末の写真は原本をそのまま渡す。手元のファイルなので、Coil が
     * 要求した大きさでデコードすれば足りる。NAS は網越しなので、
     * 準備のときに作って置いたものを使う。
     */
    fun displayModel(edge: Int): Any =
        smb?.let { SmbImage(it.nasId, it.path, SmbSize.Display, edge) }
            ?: amazon?.let { AmazonImage(it, SmbSize.Display, edge) } ?: uri

    /** 拡大して見るときの絵。原本。 */
    val fullModel: Any
        get() = smb?.let { SmbImage(it.nasId, it.path, SmbSize.Full) }
            ?: amazon?.let { AmazonImage(it, SmbSize.Full) } ?: uri
}

/** NAS の写真の指し先。**どの NAS の、どの道筋か。** */
data class SmbRef(val nasId: String, val path: String)

/**
 * Amazon の写真の指し先。**鍵は node id。** tempLink は控えとして持つだけで、
 * 使えなければ取り直す（設計 08 章 5.2）。
 */
data class AmazonRef(val shareKey: String, val nodeId: String, val tempLink: String)

private val COLLECTION = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
    MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL)
} else {
    MediaStore.Images.Media.EXTERNAL_CONTENT_URI
}

/** このアプリが扱う形式。 */
private val MIME_TYPES = arrayOf("image/jpeg", "image/png", "image/webp")

private fun mimeSelection(): Pair<String, Array<String>> {
    val placeholders = MIME_TYPES.joinToString(",") { "?" }
    return "${MediaStore.Images.Media.MIME_TYPE} IN ($placeholders)" to MIME_TYPES
}

object Photos {
    private const val TAG = "Photos"

    /** アルバム（bucket）の一覧。枚数と、見出しに使う 1 枚を添える。 */
    suspend fun albums(context: Context): List<Album> = withContext(Dispatchers.IO) {
        val (where, args) = mimeSelection()
        val projection = arrayOf(
            MediaStore.Images.Media._ID,
            MediaStore.Images.Media.BUCKET_ID,
            MediaStore.Images.Media.BUCKET_DISPLAY_NAME,
            MediaStore.Images.Media.RELATIVE_PATH
        )
        // bucket ごとの枚数は SQL の GROUP BY が使えないので、数えながら畳む。
        val found = LinkedHashMap<String, Album>()
        context.contentResolver.query(
            COLLECTION, projection, where, args,
            "${MediaStore.Images.Media.DATE_TAKEN} DESC"
        )?.use { cursor ->
            val idColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media._ID)
            val bucketColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media.BUCKET_ID)
            val nameColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media.BUCKET_DISPLAY_NAME)
            val dirColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media.RELATIVE_PATH)
            while (cursor.moveToNext()) {
                val bucket = cursor.getString(bucketColumn) ?: continue
                val name = cursor.getString(nameColumn) ?: bucket
                val photoId = cursor.getLong(idColumn)
                val prior = found[bucket]
                found[bucket] = if (prior == null) {
                    Album(bucket, name, 1, photoId, cursor.getString(dirColumn) ?: "")
                } else {
                    prior.copy(count = prior.count + 1)
                }
            }
        }
        found.values.sortedByDescending { it.count }
    }

    /**
     * 出所から写真を引く。**プロジェクトはここだけを通る。**
     * 出所の種類が増えても、上の画面はこの 1 か所しか知らなくてよい。
     */
    suspend fun forSource(context: Context, source: Source): List<Photo> = try {
        list(context, source)
    } catch (error: Exception) {
        // 画面から呼ばれる。**ここで落とさない。** 理由は準備の側（list）で残す。
        Log.w("Photos", "写真を読めなかった: ${source.kind}", error)
        emptyList()
    }

    /**
     * 読めなかったら**理由を投げる**。準備はこちらを使う（つまずきとして残すため）。
     * Amazon の「リンクが消えた」をここで拾えないと、空のプロジェクトに見える。
     */
    suspend fun list(context: Context, source: Source): List<Photo> = when (source.kind) {
        "album" -> photos(context, source.key)
        "nas" -> fromNas(context, source.key)
        "amazon" -> fromAmazon(source.key)
        else -> emptyList()
    }

    /** Amazon の共有リンクの写真。**撮影時刻の昇順で来る。** */
    private suspend fun fromAmazon(key: String): List<Photo> =
        when (val got = Amazon.photos(Amazon.linkOf(key))) {
            is SmbResult.Failed -> throw IllegalStateException(got.reason)
            is SmbResult.Ok -> fromItems(key, got.value)
        }

    /** Amazon の一覧を写真に。**作成画面で読んだものをそのまま控えるのにも使う。** */
    fun fromItems(key: String, items: List<Amazon.Item>): List<Photo> = items.map { item ->
        Photo(
            // MediaStore の id は無いので node id から作る。**同じ写真なら同じ値。**
            id = item.nodeId.hashCode().toLong() and 0xffffffffL,
            name = item.name,
            // 星と連写の鍵。**名前は重なりうるので node id にする。**
            relativePath = item.nodeId,
            size = item.size,
            takenAt = item.takenAt,
            amazon = AmazonRef(key, item.nodeId, item.tempLink)
        )
    }

    /**
     * NAS のフォルダの写真。**撮影時刻は EXIF から取る。**
     *
     * 更新時刻はコピーしたときに変わってしまい、撮影順にならない。
     * EXIF は原本の先頭 128KB に入っているので、そこだけ読む。
     * 読めたぶんは指紋と一緒に控えるので、2 回目以降は網に行かない。
     */
    private suspend fun fromNas(context: Context, key: String): List<Photo> {
        val nasId = key.substringBefore("|")
        val deep = key.endsWith("|**")
        val folder = key.removeSuffix("|**").substringAfter("|")
        val nas = NasStore.all(context).firstOrNull { it.id == nasId } ?: return emptyList()
        val password = Session.password(context, nas) ?: return emptyList()
        // **「以下ぜんぶ」なら入れ子もたどる。** 印は鍵の末尾に付いている。
        val listed = if (deep) Smb.photosDeep(nas, password, folder)
        else Smb.photos(nas, password, folder)
        if (listed !is SmbResult.Ok) return emptyList()
        return listed.value.map { entry ->
            Photo(
                // MediaStore の id は無いので、道筋から作る。**同じ道筋なら同じ値。**
                id = entry.path.hashCode().toLong() and 0xffffffffL,
                name = entry.name,
                // 共有の鍵はフォルダ内の相対の道筋。区切りは / に揃える。
                relativePath = entry.path.replace("\\", "/"),
                size = entry.size,
                takenAt = entry.modifiedAt,
                smb = SmbRef(nasId, entry.path)
            )
        }
    }

    /** あるアルバムの写真。**撮影時刻の昇順**で返す。 */
    suspend fun photos(context: Context, albumId: String): List<Photo> = withContext(Dispatchers.IO) {
        val (mimeWhere, mimeArgs) = mimeSelection()
        val where = "$mimeWhere AND ${MediaStore.Images.Media.BUCKET_ID} = ?"
        val args = mimeArgs + albumId
        val projection = arrayOf(
            MediaStore.Images.Media._ID,
            MediaStore.Images.Media.DISPLAY_NAME,
            MediaStore.Images.Media.RELATIVE_PATH,
            MediaStore.Images.Media.SIZE,
            MediaStore.Images.Media.DATE_TAKEN,
            MediaStore.Images.Media.DATE_MODIFIED
        )
        val out = ArrayList<Photo>()
        // 並べ替えは SQL に任せない。**この端末では 2,257 枚のうち 1,497 枚で
        // DATE_TAKEN が NULL** で、SQL に並べさせると mtime に落とした時刻と
        // 並び順が食い違う。連写は隣どうしでしか畳まないので、順が違えば
        // どれだけ似ていてもまとまらない。実際にまとまり 0 になっていた。
        context.contentResolver.query(
            COLLECTION, projection, where, args, null
        )?.use { cursor ->
            val idColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media._ID)
            val nameColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media.DISPLAY_NAME)
            val pathColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media.RELATIVE_PATH)
            val sizeColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media.SIZE)
            val takenColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media.DATE_TAKEN)
            val modifiedColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media.DATE_MODIFIED)
            while (cursor.moveToNext()) {
                val name = cursor.getString(nameColumn) ?: "photo.jpg"
                // DATE_TAKEN が無い写真がある（スキャンした画像など）。
                // **無いものを 0 にすると全部が同じ時刻になる**ので mtime に落とす。
                val taken = cursor.getLong(takenColumn).takeIf { it > 0 }
                    ?: (cursor.getLong(modifiedColumn) * 1000L)
                out += Photo(
                    id = cursor.getLong(idColumn),
                    name = name,
                    relativePath = (cursor.getString(pathColumn) ?: "") + name,
                    size = cursor.getLong(sizeColumn),
                    takenAt = taken
                )
            }
        }
        // **使う値そのもので並べる。** 同時刻のものは名前で決める（毎回同じ順）。
        out.sortedWith(compareBy({ it.takenAt }, { it.relativePath }))
    }

    /**
     * OS が持っているサムネイル。**自分で作らない。**
     *
     * `loadThumbnail` は MediaStore 側の縮小画像を返す（無ければ作る）。
     * Tauri 版は原本を読んで自分で縮小していたが、それは 1 枚 6.7MB を
     * 読むということで、2,000 枚では 13GB になる。ここが native の一番の利点。
     */
    fun thumbnail(context: Context, photo: Photo, edge: Int = 512): Bitmap? = try {
        context.contentResolver.loadThumbnail(photo.uri, Size(edge, edge), null)
    } catch (error: Exception) {
        // 消された・壊れている。1 枚で全体を止めない。**理由は必ず残す。**
        Log.w(TAG, "サムネイルを読めない: ${photo.name}", error)
        null
    }

    /**
     * `loadThumbnail` の速さを測る。**「自分で作る」をやめられるかの判断材料。**
     * 結果は logcat に出す。
     */
    suspend fun measureThumbnails(
        context: Context,
        photos: List<Photo>,
        edge: Int
    ): String = withContext(Dispatchers.IO) {
        val sample = photos.take(30)
        if (sample.isEmpty()) return@withContext "写真がありません"
        var ok = 0
        var bytes = 0L
        val started = System.nanoTime()
        for (photo in sample) {
            val bitmap = thumbnail(context, photo, edge)
            if (bitmap != null) {
                ok += 1
                bytes += bitmap.byteCount.toLong()
                bitmap.recycle()
            }
        }
        val elapsedMs = (System.nanoTime() - started) / 1_000_000.0
        val each = elapsedMs / sample.size
        val line = "loadThumbnail(${edge}px): ${sample.size} 枚を ${"%.0f".format(elapsedMs)}ms" +
            "（1 枚 ${"%.1f".format(each)}ms、成功 $ok、平均 ${bytes / maxOf(1, ok) / 1024}KB）"
        Log.i(TAG, line)
        line
    }
}
