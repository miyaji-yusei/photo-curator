package app.photocurator.next

/**
 * RAW に埋め込まれたプレビュー JPEG を探す（U49）。**Android の標準のデコーダは RAW を読めない**ので、
 * カメラが RAW の中に入れておいた JPEG を取り出して、それを絵として使う。
 *
 * 純関数（android.* を使わない）。読み方は [ByteSource] で受け取るので、端末のファイルも NAS も
 * 同じ道を通る。**RAW は 25MB 級なので全部は読まない。** IFD をたどって、要る範囲だけを読む。
 *
 * 対応する形:
 *  - TIFF 系（CR2・NEF・ARW・DNG・PEF・SRW・ORF・RW2）: IFD0・IFD1…の鎖、SubIFD、
 *    `JPEGInterchangeFormat`（0x0201/0x0202）、圧縮が JPEG（6・7）の 1 本だけの strip、
 *    RW2 の `JpgFromRaw`（0x002E）。
 *  - RAF: 先頭の見出しに書かれた JPEG の位置。
 *  - CR3（ISOBMFF）: 最上位の uuid 箱の中の `PRVW`。向きは moov の中の `CMT1`（TIFF の IFD0）。
 *
 * JPEG かどうかは**中身で決める。** 先頭が SOI で、SOF が基本（C0）・拡張（C1）・プログレッシブ（C2）の
 * ものだけ。CR2・DNG の RAW 本体は可逆 JPEG（C3）なので、ここで落ちる。
 */
fun interface ByteSource {
    /** `offset` から最大 `length` バイト。ファイルの外なら null、終わりにかかれば短く返す。 */
    fun read(offset: Long, length: Int): ByteArray?
}

/** バイト列そのもの。テストと、もう読んである先頭に使う。 */
class ArraySource(private val bytes: ByteArray) : ByteSource {
    override fun read(offset: Long, length: Int): ByteArray? {
        if (offset < 0 || length < 0 || offset >= bytes.size) return null
        val end = minOf(bytes.size.toLong(), offset + length).toInt()
        return bytes.copyOfRange(offset.toInt(), end)
    }
}

/**
 * 区画ごとに覚えて読む。**IFD をたどると小さな読みが何十回も起きる**ので、網越しでは
 * 1 回ごとに往復させない。大きな読み（プレビューの本体）は覚えずにそのまま通す。
 */
class BlockCache(
    private val source: ByteSource,
    private val blockSize: Int = 64 * 1024,
    private val maxBlocks: Int = 16
) : ByteSource {
    private val blocks = object : LinkedHashMap<Long, ByteArray>(maxBlocks, 0.75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<Long, ByteArray>?) =
            size > maxBlocks
    }

    private fun block(index: Long): ByteArray? =
        blocks[index] ?: source.read(index * blockSize, blockSize)?.also { blocks[index] = it }

    override fun read(offset: Long, length: Int): ByteArray? {
        if (offset < 0 || length < 0) return null
        if (length > blockSize * 4) return source.read(offset, length)
        val out = java.io.ByteArrayOutputStream(length)
        var at = offset
        val end = offset + length
        while (at < end) {
            val index = at / blockSize
            val data = block(index) ?: break
            val inside = (at - index * blockSize).toInt()
            if (inside >= data.size) break
            val take = minOf(data.size - inside, (end - at).toInt())
            out.write(data, inside, take)
            at += take
            if (data.size < blockSize) break
        }
        return if (out.size() == 0) null else out.toByteArray()
    }
}

/** 埋め込みの JPEG 1 つ。**大きさは JPEG の SOF から読んだもの。** */
data class JpegRegion(val offset: Long, val length: Int, val width: Int, val height: Int) {
    val longEdge: Int get() = maxOf(width, height)
}

/**
 * RAW を見て分かったこと。`orientation` は EXIF の値（1〜8）、分からなければ null。
 * `takenAt` は `DateTimeOriginal` の文字列（"yyyy:MM:dd HH:mm:ss"）、無ければ null。
 */
data class RawInfo(val previews: List<JpegRegion>, val orientation: Int?, val takenAt: String?)

object RawPreview {
    /** 「大きいプレビュー」の目安。これ以上なら表示用・拡大に使える。 */
    const val LARGE = 1000

    private val EMPTY = RawInfo(emptyList(), null, null)

    /** CR3 のプレビューの入った uuid 箱。 */
    private val PRVW_UUID = hex("eaf42b5e1c984b88b9fbb7dc406e4d16")

    /** CR3 の moov の中の、CMT1〜（TIFF）の入った uuid 箱。 */
    private val CMT_UUID = hex("85c0b687820f11e08111f4ce462b6a48")

    private const val MAX_IFDS = 64
    private const val MAX_ENTRIES = 1000
    private const val MAX_SEGMENTS = 200

    private fun hex(text: String) = text.chunked(2).map { it.toInt(16).toByte() }.toByteArray()

    /** 読めなくても投げない。**分からなければ空。**（呼ぶ側は「表示できません」に倒す） */
    fun inspect(source: ByteSource): RawInfo = try {
        inspectOrThrow(source)
    } catch (error: Exception) {
        EMPTY
    }

    private fun inspectOrThrow(source: ByteSource): RawInfo {
        val head = source.read(0, 32) ?: return EMPTY
        if (head.size < 16) return EMPTY
        val text = String(head, 0, minOf(head.size, 16), Charsets.ISO_8859_1)
        return when {
            text.startsWith("FUJIFILMCCD-RAW") -> raf(source)
            head.size >= 8 && String(head, 4, 4, Charsets.ISO_8859_1) == "ftyp" -> cr3(source)
            text.startsWith("II") || text.startsWith("MM") -> tiff(source, 0L)
            else -> EMPTY
        }
    }

    /** 一番大きいもの。**表示用画像と拡大の元。** */
    fun largest(info: RawInfo): JpegRegion? =
        info.previews.maxWithOrNull(compareBy<JpegRegion>({ it.width.toLong() * it.height }, { it.length }))

    /**
     * サムネイル用。長辺が `minEdge` 以上の中で一番小さいもの（網越しに読む量を減らす）。
     * 足りるものが無ければ一番大きいもの。
     */
    fun forThumb(info: RawInfo, minEdge: Int = 160): JpegRegion? =
        info.previews.filter { it.longEdge >= minEdge }
            .minWithOrNull(compareBy<JpegRegion>({ it.width.toLong() * it.height }, { it.length }))
            ?: largest(info)

    /** JPEG の本体を読む。**全部読めなければ null**（途中で切れた JPEG は渡さない）。 */
    fun bytesOf(source: ByteSource, region: JpegRegion): ByteArray? {
        val bytes = source.read(region.offset, region.length) ?: return null
        return if (bytes.size == region.length) bytes else null
    }

    // ---- TIFF ----

    private class Reader(val source: ByteSource, val little: Boolean, val base: Long) {
        fun bytes(at: Long, length: Int): ByteArray? =
            source.read(base + at, length)?.takeIf { it.size == length }

        fun u16(at: Long): Int? = bytes(at, 2)?.let { u16(it, 0) }
        fun u32(at: Long): Long? = bytes(at, 4)?.let { u32(it, 0) }

        fun u16(data: ByteArray, at: Int): Int {
            val a = data[at].toInt() and 0xFF
            val b = data[at + 1].toInt() and 0xFF
            return if (little) a or (b shl 8) else (a shl 8) or b
        }

        fun u32(data: ByteArray, at: Int): Long {
            var value = 0L
            for (i in 0 until 4) {
                val byte = (data[at + i].toLong() and 0xFF)
                value = if (little) value or (byte shl (8 * i)) else (value shl 8) or byte
            }
            return value
        }
    }

    private enum class Kind { Chain, Sub, Exif }

    /** `base` から始まる TIFF を読む（CR3 の CMT1 は箱の中にあるので、そこからの相対）。 */
    private fun tiff(source: ByteSource, base: Long, previewsToo: Boolean = true): RawInfo {
        val header = source.read(base, 8)?.takeIf { it.size == 8 } ?: return EMPTY
        val little = when {
            header[0] == 'I'.code.toByte() && header[1] == 'I'.code.toByte() -> true
            header[0] == 'M'.code.toByte() && header[1] == 'M'.code.toByte() -> false
            else -> return EMPTY
        }
        val r = Reader(source, little, base)
        val first = r.u32(4) ?: return EMPTY
        val queue = ArrayDeque<Pair<Long, Kind>>()
        queue.add(first to Kind.Chain)
        val visited = HashSet<Long>()
        val found = ArrayList<Pair<Long, Long>>()
        var orientation: Int? = null
        var takenAt: String? = null
        var isFirst = true
        while (queue.isNotEmpty() && visited.size < MAX_IFDS) {
            val (at, kind) = queue.removeFirst()
            if (at <= 0 || !visited.add(at)) continue
            val count = r.u16(at) ?: continue
            if (count == 0 || count > MAX_ENTRIES) continue
            val table = r.bytes(at + 2, count * 12) ?: continue
            var compression = 0
            var stripOffset = 0L
            var stripLength = 0L
            var stripCount = 0L
            var jpegOffset = 0L
            var jpegLength = 0L
            for (i in 0 until count) {
                val e = i * 12
                val tag = r.u16(table, e)
                val type = r.u16(table, e + 2)
                val n = r.u32(table, e + 4)
                val short = r.u16(table, e + 8)
                val long = r.u32(table, e + 8)
                val scalar = if (type == 3) short.toLong() else long
                when (tag) {
                    0x0103 -> compression = scalar.toInt()
                    0x0111 -> { stripOffset = scalar; stripCount = n }
                    0x0117 -> stripLength = scalar
                    0x0112 -> if (isFirst && kind == Kind.Chain) orientation = short.takeIf { it in 1..8 }
                    0x0201 -> jpegOffset = scalar
                    0x0202 -> jpegLength = scalar
                    0x002E -> if (type == 7 && n > 4) found += long to n
                    0x014A -> if (n in 1..16) {
                        if (n == 1L) queue.add(long to Kind.Sub)
                        else r.bytes(long, (n * 4).toInt())?.let { list ->
                            for (k in 0 until n.toInt()) queue.add(r.u32(list, k * 4) to Kind.Sub)
                        }
                    }
                    0x8769 -> queue.add(long to Kind.Exif)
                    0x9003 -> if (kind == Kind.Exif && type == 2 && n in 5..64) {
                        takenAt = r.bytes(long, n.toInt())
                            ?.toString(Charsets.ISO_8859_1)?.trimEnd('\u0000', ' ')
                            ?.takeIf { it.isNotEmpty() }
                    }
                }
            }
            if (jpegOffset > 0 && jpegLength > 0) found += jpegOffset to jpegLength
            if ((compression == 6 || compression == 7) && stripCount == 1L && stripOffset > 0 && stripLength > 0) {
                found += stripOffset to stripLength
            }
            if (kind == Kind.Chain) {
                r.u32(at + 2 + count * 12L)?.takeIf { it > 0 }?.let { queue.add(it to Kind.Chain) }
            }
            isFirst = false
        }
        val previews = if (previewsToo) regions(source, found.map { (o, l) -> base + o to l }) else emptyList()
        return RawInfo(previews, orientation, takenAt)
    }

    /** 候補を JPEG として確かめる。**同じ位置は 1 つにする。** */
    private fun regions(source: ByteSource, candidates: List<Pair<Long, Long>>): List<JpegRegion> =
        candidates
            .filter { (_, length) -> length in 4..Int.MAX_VALUE.toLong() }
            .distinctBy { it.first }
            .mapNotNull { (offset, length) ->
                // **ファイルの外を指すものは捨てる。** 最後の 1 バイトが読めるか。
                val tail = source.read(offset + length - 1, 1)
                if (tail == null || tail.isEmpty()) return@mapNotNull null
                val (width, height) = jpegSize(source, offset, length) ?: return@mapNotNull null
                JpegRegion(offset, length.toInt(), width, height)
            }

    /**
     * JPEG の大きさ（幅, 高さ）。**区切り（マーカー）を飛ばしながら SOF を探す。**
     * 基本・拡張・プログレッシブ以外（可逆・算術符号など）は null。
     */
    fun jpegSize(source: ByteSource, offset: Long, length: Long): Pair<Int, Int>? {
        val soi = source.read(offset, 2) ?: return null
        if (soi.size < 2 || soi[0] != 0xFF.toByte() || soi[1] != 0xD8.toByte()) return null
        var at = offset + 2
        val end = offset + length
        repeat(MAX_SEGMENTS) {
            if (at + 4 > end) return null
            val head = source.read(at, 4)?.takeIf { it.size == 4 } ?: return null
            if (head[0] != 0xFF.toByte()) return null
            val marker = head[1].toInt() and 0xFF
            when {
                marker == 0xFF -> { at += 1; return@repeat }
                marker in 0xD0..0xD7 || marker == 0x01 -> { at += 2; return@repeat }
                marker == 0xD9 || marker == 0xDA -> return null
            }
            val segment = ((head[2].toInt() and 0xFF) shl 8) or (head[3].toInt() and 0xFF)
            if (segment < 2) return null
            when (marker) {
                0xC0, 0xC1, 0xC2 -> {
                    val sof = source.read(at + 5, 4)?.takeIf { it.size == 4 } ?: return null
                    val height = ((sof[0].toInt() and 0xFF) shl 8) or (sof[1].toInt() and 0xFF)
                    val width = ((sof[2].toInt() and 0xFF) shl 8) or (sof[3].toInt() and 0xFF)
                    return if (width > 0 && height > 0) width to height else null
                }
                0xC3, 0xC5, 0xC6, 0xC7, 0xC9, 0xCA, 0xCB, 0xCD, 0xCE, 0xCF -> return null
            }
            at += 2L + segment
        }
        return null
    }

    // ---- RAF ----

    private fun raf(source: ByteSource): RawInfo {
        val r = Reader(source, little = false, base = 0)
        val offset = r.u32(84) ?: return EMPTY
        val length = r.u32(88) ?: return EMPTY
        return RawInfo(regions(source, listOf(offset to length)), null, null)
    }

    // ---- CR3（ISOBMFF） ----

    private data class Box(val at: Long, val type: String, val header: Int, val size: Long)

    /** `from` から `until` までの箱を並べる。**数と大きさに上限を付ける。** */
    private fun boxes(source: ByteSource, from: Long, until: Long?): List<Box> {
        val out = ArrayList<Box>()
        var at = from
        val r = Reader(source, little = false, base = 0)
        while (out.size < 64 && (until == null || at + 8 <= until)) {
            val head = source.read(at, 16) ?: break
            if (head.size < 8) break
            var size = r.u32(head, 0)
            val type = String(head, 4, 4, Charsets.ISO_8859_1)
            var header = 8
            if (size == 1L) {
                if (head.size < 16) break
                size = (r.u32(head, 8) shl 32) or r.u32(head, 12)
                header = 16
            }
            if (size == 0L) {
                out += Box(at, type, header, Long.MAX_VALUE / 4)
                break
            }
            if (size < header) break
            out += Box(at, type, header, size)
            at += size
        }
        return out
    }

    private fun isUuid(source: ByteSource, box: Box, id: ByteArray): Boolean =
        box.type == "uuid" && source.read(box.at + box.header, 16)?.contentEquals(id) == true

    private fun cr3(source: ByteSource): RawInfo {
        val top = boxes(source, 0, null)
        val found = ArrayList<Pair<Long, Long>>()
        var orientation: Int? = null
        var takenAt: String? = null
        for (box in top) {
            if (isUuid(source, box, PRVW_UUID)) {
                // 中身の先頭に PRVW がある。間の詰め物の長さは機種で違いうるので、探す。
                val start = box.at + box.header + 16
                val window = source.read(start, 256) ?: continue
                val index = indexOf(window, "PRVW".toByteArray(Charsets.ISO_8859_1))
                if (index < 4) continue
                val prvw = start + index - 4
                val r = Reader(source, little = false, base = 0)
                val length = r.u32(prvw + 20) ?: continue
                found += (prvw + 24) to length
            } else if (box.type == "moov") {
                for (child in boxes(source, box.at + box.header, box.at + box.size)) {
                    if (!isUuid(source, child, CMT_UUID)) continue
                    for (cmt in boxes(source, child.at + child.header + 16, child.at + child.size)) {
                        val inner = cmt.at + cmt.header
                        when (cmt.type) {
                            "CMT1" -> orientation = tiff(source, inner, previewsToo = false).orientation
                            // CMT2 は EXIF の IFD を TIFF の形で持つ。
                            "CMT2" -> takenAt = exifDate(source, inner)
                        }
                    }
                }
            }
        }
        return RawInfo(regions(source, found), orientation, takenAt)
    }

    /** TIFF の形の EXIF の IFD（CR3 の CMT2）から撮影時刻。 */
    private fun exifDate(source: ByteSource, base: Long): String? {
        val header = source.read(base, 8)?.takeIf { it.size == 8 } ?: return null
        val little = header[0] == 'I'.code.toByte()
        val r = Reader(source, little, base)
        val at = r.u32(4) ?: return null
        val count = r.u16(at)?.takeIf { it in 1..MAX_ENTRIES } ?: return null
        val table = r.bytes(at + 2, count * 12) ?: return null
        for (i in 0 until count) {
            val e = i * 12
            if (r.u16(table, e) != 0x9003) continue
            val n = r.u32(table, e + 4)
            if (n !in 5..64) return null
            return r.bytes(r.u32(table, e + 8), n.toInt())
                ?.toString(Charsets.ISO_8859_1)?.trimEnd('\u0000', ' ')?.takeIf { it.isNotEmpty() }
        }
        return null
    }

    private fun indexOf(data: ByteArray, needle: ByteArray): Int {
        outer@ for (i in 0..data.size - needle.size) {
            for (j in needle.indices) if (data[i + j] != needle[j]) continue@outer
            return i
        }
        return -1
    }
}
