package app.photocurator.next

import uniffi.photo_curator_core.isRawName
import uniffi.photo_curator_core.pairedRawMask
import uniffi.photo_curator_core.rawExtensions

/**
 * RAW のファイルの扱い（U49）。**RAW の判定と「組の RAW を除く」規則は core が持つ（R10）。**
 * PC（`skip_paired_raw`）・Web（`skipPairedRaw`）も同じ core を呼ぶ。
 *
 * 規則が 3 台でずれると、同じフォルダでも写真の顔ぶれが変わり、星と連写の鍵が噛み合わない。
 * だから Android の都合で足したり引いたりしない。
 */
object RawFiles {
    /** RAW の拡張子（小文字）。core の `RAW_EXTENSIONS` の 10 種。 */
    val EXTENSIONS: Set<String> = rawExtensions().toSet()

    /** 道筋の最後の部分（ファイル名）。区切りは / でも \ でもよい。 */
    private fun fileName(path: String): String =
        path.substring(maxOf(path.lastIndexOf('/'), path.lastIndexOf('\\')) + 1)

    /**
     * 拡張子（小文字）。Rust の `Path::extension` と同じく、**先頭の点は拡張子にしない**
     * （".cr2" は拡張子なし）。MIME を引くのに使う。
     */
    fun extensionOf(path: String): String? {
        val name = fileName(path)
        val dot = name.lastIndexOf('.')
        return if (dot <= 0) null else name.substring(dot + 1).lowercase()
    }

    /** 拡張子が RAW か（core の `is_raw_name`）。 */
    fun isRaw(path: String): Boolean = isRawName(path)

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

    /**
     * RAW＋JPEG 同時撮影の「組」の RAW を除く。`enabled` はプロジェクトの設定（`pairRawJpeg`）。
     * false なら何も除かない（RAW も全部対象）。並びは変えない。
     *
     * 除かないもの: 組の JPEG が無い RAW、別フォルダの同名、HEIC・HEIF、PNG・WebP との組、RAW 同士。
     * 規則は core の `paired_raw_mask`。ここは並びを保って絞るだけ。
     */
    fun <T> skipPairedRaw(items: List<T>, enabled: Boolean, pathOf: (T) -> String): List<T> {
        if (!enabled) return items
        val mask = pairedRawMask(items.map(pathOf))
        return items.filterIndexed { index, _ -> !mask[index] }
    }
}
