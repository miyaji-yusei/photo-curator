package app.photocurator.next

/**
 * RAW のファイルの扱い（U49）。**PC（U46 の `skip_paired_raw`）・Web（`skipPairedRaw`）と同じ規則。**
 *
 * 規則が 3 台でずれると、同じフォルダでも写真の顔ぶれが変わり、星と連写の鍵が噛み合わない。
 * だから Android の都合で足したり引いたりしない。
 */
object RawFiles {
    /** RAW の拡張子。PC の `RAW_EXTENSIONS` と同じ 10 種。 */
    val EXTENSIONS = setOf("cr2", "cr3", "nef", "arw", "dng", "raf", "orf", "rw2", "pef", "srw")

    /** 組になる JPEG の拡張子。**HEIC・PNG・WebP は組にしない。** */
    private val JPEG = setOf("jpg", "jpeg")

    /** 道筋の最後の部分（ファイル名）。区切りは / でも \ でもよい。 */
    private fun fileName(path: String): String =
        path.substring(maxOf(path.lastIndexOf('/'), path.lastIndexOf('\\')) + 1)

    /**
     * 拡張子（小文字）。Rust の `Path::extension` と同じく、**先頭の点は拡張子にしない**
     * （".cr2" は拡張子なし）。
     */
    fun extensionOf(path: String): String? {
        val name = fileName(path)
        val dot = name.lastIndexOf('.')
        return if (dot <= 0) null else name.substring(dot + 1).lowercase()
    }

    fun isRaw(path: String): Boolean = extensionOf(path) in EXTENSIONS

    /**
     * RAW の MIME。**MediaStore がこの名前で返す**（Android の MediaFile と同じ）。
     * 端末によっては application/octet-stream で入っていることもあるので、
     * 一覧では拡張子でも拾う（[Photos] の選び方）。
     */
    val MIME_TYPES = mapOf(
        "cr2" to "image/x-canon-cr2",
        "cr3" to "image/x-canon-cr3",
        "nef" to "image/x-nikon-nef",
        "arw" to "image/x-sony-arw",
        "dng" to "image/x-adobe-dng",
        "raf" to "image/x-fuji-raf",
        "orf" to "image/x-olympus-orf",
        "rw2" to "image/x-panasonic-rw2",
        "pef" to "image/x-pentax-pef",
        "srw" to "image/x-samsung-srw"
    )

    /** RAW なら MIME、そうでなければ null。 */
    fun mimeOf(path: String): String? = extensionOf(path)?.let { MIME_TYPES[it] }

    /** 組を見分ける鍵。**同じフォルダ**で、拡張子を除いた名前（大文字小文字を無視）。 */
    private fun keyOf(path: String): Pair<String, String> {
        val cut = maxOf(path.lastIndexOf('/'), path.lastIndexOf('\\'))
        val folder = if (cut < 0) "" else path.substring(0, cut).replace('\\', '/')
        val name = fileName(path)
        val dot = name.lastIndexOf('.')
        val stem = if (dot <= 0) name else name.substring(0, dot)
        return folder to stem.lowercase()
    }

    /**
     * RAW＋JPEG 同時撮影の「組」の RAW を除く。`enabled` はプロジェクトの設定（`pairRawJpeg`）。
     * false なら何も除かない（RAW も全部対象）。並びは変えない。
     *
     * 除かないもの: 組の JPEG が無い RAW、別フォルダの同名、HEIC・HEIF、PNG・WebP との組、RAW 同士。
     */
    fun <T> skipPairedRaw(items: List<T>, enabled: Boolean, pathOf: (T) -> String): List<T> {
        if (!enabled) return items
        val jpegKeys = items.asSequence()
            .map(pathOf)
            .filter { extensionOf(it) in JPEG }
            .map(::keyOf)
            .toHashSet()
        if (jpegKeys.isEmpty()) return items
        return items.filterNot { item ->
            val path = pathOf(item)
            isRaw(path) && keyOf(path) in jpegKeys
        }
    }
}
