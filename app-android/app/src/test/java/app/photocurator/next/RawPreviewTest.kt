package app.photocurator.next

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * U49: RAW に埋め込まれたプレビュー JPEG を探す。**手作りの小さなバイト列で確かめる。**
 * 本物の RAW は 25MB 級なので置かない。形（IFD のたどり方・JPEG の見分け方）だけを見る。
 */
class RawPreviewTest {

    /** 決めた位置に値を書く。**TIFF の並びを手で組むため。** */
    private class Bytes(size: Int, private val little: Boolean) {
        val data = ByteArray(size)
        fun u16(at: Int, value: Int) {
            if (little) { data[at] = value.toByte(); data[at + 1] = (value shr 8).toByte() }
            else { data[at] = (value shr 8).toByte(); data[at + 1] = value.toByte() }
        }
        fun u32(at: Int, value: Long) {
            for (i in 0 until 4) {
                val shift = if (little) 8 * i else 8 * (3 - i)
                data[at + i] = (value shr shift).toByte()
            }
        }
        fun put(at: Int, bytes: ByteArray) = bytes.copyInto(data, at)
        fun text(at: Int, value: String) = put(at, value.toByteArray(Charsets.US_ASCII))

        /** IFD を 1 つ。entries は (タグ, 型, 個数, 値)。SHORT 1 個は値の欄の先頭 2 バイト。 */
        fun ifd(at: Int, entries: List<IntArray>, next: Long = 0) {
            u16(at, entries.size)
            entries.forEachIndexed { i, (tag, type, count, value) ->
                val base = at + 2 + 12 * i
                u16(base, tag); u16(base + 2, type); u32(base + 4, count.toLong())
                if (type == 3 && count == 1) { u32(base + 8, 0); u16(base + 8, value) }
                else u32(base + 8, value.toLong())
            }
            u32(at + 2 + 12 * entries.size, next)
        }
    }

    /** それらしい JPEG。SOI・APP0・DHT・SOF・EOI。`sof` は 0xC0（基本）など。 */
    private fun jpeg(width: Int, height: Int, size: Int, sof: Int = 0xC0, app1: Int = 0): ByteArray {
        val out = java.io.ByteArrayOutputStream()
        out.write(byteArrayOf(0xFF.toByte(), 0xD8.toByte()))
        out.write(byteArrayOf(0xFF.toByte(), 0xE0.toByte(), 0, 16)); out.write(ByteArray(14))
        if (app1 > 0) {
            out.write(byteArrayOf(0xFF.toByte(), 0xE1.toByte(), (app1 shr 8).toByte(), app1.toByte()))
            out.write(ByteArray(app1 - 2))
        }
        // DHT（0xC4）は SOF ではない。**取り違えない**ことを確かめる。
        out.write(byteArrayOf(0xFF.toByte(), 0xC4.toByte(), 0, 5, 0, 0, 0))
        out.write(byteArrayOf(0xFF.toByte(), sof.toByte(), 0, 17, 8,
            (height shr 8).toByte(), height.toByte(), (width shr 8).toByte(), width.toByte(), 3))
        out.write(ByteArray(9))
        val body = out.toByteArray()
        require(body.size + 2 <= size)
        return body + ByteArray(size - body.size - 2) + byteArrayOf(0xFF.toByte(), 0xD9.toByte())
    }

    private val SHORT = 3
    private val LONG = 4
    private val ASCII = 2
    private val UNDEFINED = 7

    /** CR2 に似た形。IFD0 の strip がプレビュー、IFD1 がサムネイル、SubIFD が可逆 JPEG（RAW 本体）。 */
    private fun cr2Like(): Bytes {
        val big = jpeg(2000, 1333, 1500)
        val thumb = jpeg(160, 120, 200)
        val lossless = jpeg(6000, 4000, 2500, sof = 0xC3)
        val b = Bytes(6600, little = true)
        b.text(0, "II"); b.u16(2, 42); b.u32(4, 8)
        b.ifd(8, listOf(
            intArrayOf(0x0103, SHORT, 1, 6),
            intArrayOf(0x0111, LONG, 1, 1000),
            intArrayOf(0x0112, SHORT, 1, 6),
            intArrayOf(0x0117, LONG, 1, big.size),
            intArrayOf(0x8769, LONG, 1, 400),
            intArrayOf(0x014A, LONG, 1, 300)
        ), next = 200)
        b.ifd(200, listOf(intArrayOf(0x0201, LONG, 1, 3000), intArrayOf(0x0202, LONG, 1, thumb.size)))
        b.ifd(300, listOf(
            intArrayOf(0x0103, SHORT, 1, 6),
            intArrayOf(0x0111, LONG, 1, 4000),
            intArrayOf(0x0117, LONG, 1, lossless.size)
        ))
        b.ifd(400, listOf(intArrayOf(0x9003, ASCII, 20, 500)))
        b.text(500, "2024:05:06 07:08:09\u0000")
        b.put(1000, big); b.put(3000, thumb); b.put(4000, lossless)
        return b
    }

    @Test fun CR2に似た形から大きいプレビューと小さいサムネイルを見つける() {
        val info = RawPreview.inspect(ArraySource(cr2Like().data))
        assertEquals(setOf(1000L, 3000L), info.previews.map { it.offset }.toSet())
        val largest = RawPreview.largest(info)!!
        assertEquals(1000L, largest.offset)
        assertEquals(2000, largest.width); assertEquals(1333, largest.height)
        assertEquals(1500, largest.length)
        assertEquals(3000L, RawPreview.forThumb(info)!!.offset)
        assertEquals(6, info.orientation)
        assertEquals("2024:05:06 07:08:09", info.takenAt)
    }

    @Test fun 可逆JPEGのRAW本体はプレビューに数えない() {
        val info = RawPreview.inspect(ArraySource(cr2Like().data))
        assertTrue(info.previews.none { it.offset == 4000L })
    }

    @Test fun 取り出したバイト列はJPEGそのもの() {
        val b = cr2Like()
        val source = ArraySource(b.data)
        val got = RawPreview.bytesOf(source, RawPreview.largest(RawPreview.inspect(source))!!)!!
        assertArrayEquals(b.data.copyOfRange(1000, 2500), got)
    }

    @Test fun ビッグエンディアンのSubIFDの配列をたどる_NEFに似た形() {
        val preview = jpeg(1024, 683, 800, sof = 0xC2)
        val b = Bytes(3000, little = false)
        b.text(0, "MM"); b.u16(2, 42); b.u32(4, 8)
        b.ifd(8, listOf(intArrayOf(0x0112, SHORT, 1, 8), intArrayOf(0x014A, LONG, 2, 100)))
        b.u32(100, 200); b.u32(104, 300)
        b.ifd(200, listOf(intArrayOf(0x0201, LONG, 1, 1000), intArrayOf(0x0202, LONG, 1, preview.size)))
        // Nikon の圧縮（34713）は JPEG ではない。
        b.ifd(300, listOf(
            intArrayOf(0x0103, SHORT, 1, 34713),
            intArrayOf(0x0111, LONG, 1, 2000),
            intArrayOf(0x0117, LONG, 1, 500)
        ))
        b.put(1000, preview)
        val info = RawPreview.inspect(ArraySource(b.data))
        assertEquals(listOf(1000L), info.previews.map { it.offset })
        assertEquals(1024, RawPreview.largest(info)!!.width)
        assertEquals(8, info.orientation)
    }

    @Test fun RW2のJpgFromRawを見つける() {
        val preview = jpeg(1920, 1080, 900)
        val b = Bytes(2000, little = true)
        b.text(0, "II"); b.u16(2, 0x55); b.u32(4, 8)
        b.ifd(8, listOf(intArrayOf(0x002E, UNDEFINED, preview.size, 1000)))
        b.put(1000, preview)
        val largest = RawPreview.largest(RawPreview.inspect(ArraySource(b.data)))!!
        assertEquals(1000L, largest.offset); assertEquals(1920, largest.width)
    }

    @Test fun RAFの先頭に書かれたJPEGを見つける() {
        val preview = jpeg(1920, 1280, 700)
        val b = Bytes(1000, little = false)
        b.text(0, "FUJIFILMCCD-RAW 0201FF383501")
        b.u32(84, 200); b.u32(88, preview.size.toLong())
        b.put(200, preview)
        val largest = RawPreview.largest(RawPreview.inspect(ArraySource(b.data)))!!
        assertEquals(200L, largest.offset); assertEquals(1280, largest.height)
    }

    @Test fun CR3のPRVWを見つける() {
        val preview = jpeg(1620, 1080, 600)
        val b = Bytes(2000, little = false)
        // ftyp（24）・moov（16、中身は空）・uuid（PRVW を含む）
        b.u32(0, 24); b.text(4, "ftyp"); b.text(8, "crx ")
        b.u32(24, 16); b.text(28, "moov")
        val uuidAt = 40
        val prvwAt = uuidAt + 8 + 16 + 8
        val uuidSize = (prvwAt - uuidAt) + 24 + preview.size
        b.u32(uuidAt.toLong().toInt(), uuidSize.toLong()); b.text(uuidAt + 4, "uuid")
        val id = "eaf42b5e1c984b88b9fbb7dc406e4d16".chunked(2).map { it.toInt(16).toByte() }.toByteArray()
        b.put(uuidAt + 8, id)
        b.u32(prvwAt, (24 + preview.size).toLong()); b.text(prvwAt + 4, "PRVW")
        b.u16(prvwAt + 12, 1); b.u16(prvwAt + 14, 1620); b.u16(prvwAt + 16, 1080); b.u16(prvwAt + 18, 1)
        b.u32(prvwAt + 20, preview.size.toLong())
        b.put(prvwAt + 24, preview)
        val largest = RawPreview.largest(RawPreview.inspect(ArraySource(b.data)))!!
        assertEquals((prvwAt + 24).toLong(), largest.offset)
        assertEquals(1620, largest.width); assertEquals(preview.size, largest.length)
    }

    @Test fun CR3の向きと撮影時刻をmoovのCMT1とCMT2から読む() {
        val b = Bytes(600, little = false)
        b.u32(0, 24); b.text(4, "ftyp"); b.text(8, "crx ")
        // moov ＞ uuid(85c0b687…) ＞ CMT1（TIFF の IFD0）・CMT2（EXIF の IFD）
        val cmt1 = 24 + 8 + 8 + 16
        val cmt1Size = 8 + 8 + 2 + 12 + 4
        val cmt2 = cmt1 + cmt1Size
        val cmt2Size = 8 + 8 + 2 + 12 + 4 + 20
        val uuidSize = 8 + 16 + cmt1Size + cmt2Size
        b.u32(24, (8 + uuidSize).toLong()); b.text(28, "moov")
        b.u32(32, uuidSize.toLong()); b.text(36, "uuid")
        b.put(40, "85c0b687820f11e08111f4ce462b6a48".chunked(2).map { it.toInt(16).toByte() }.toByteArray())
        b.u32(cmt1, cmt1Size.toLong()); b.text(cmt1 + 4, "CMT1")
        val t1 = cmt1 + 8
        b.text(t1, "MM"); b.u16(t1 + 2, 42); b.u32(t1 + 4, 8)
        b.ifd(t1 + 8, listOf(intArrayOf(0x0112, SHORT, 1, 8)))
        b.u32(cmt2, cmt2Size.toLong()); b.text(cmt2 + 4, "CMT2")
        val t2 = cmt2 + 8
        b.text(t2, "MM"); b.u16(t2 + 2, 42); b.u32(t2 + 4, 8)
        b.ifd(t2 + 8, listOf(intArrayOf(0x9003, ASCII, 20, 8 + 2 + 12 + 4)))
        b.text(t2 + 8 + 2 + 12 + 4, "2023:01:02 03:04:05\u0000")
        val info = RawPreview.inspect(ArraySource(b.data))
        assertEquals(8, info.orientation)
        assertEquals("2023:01:02 03:04:05", info.takenAt)
    }

    @Test fun 輪になったIFDでも止まる() {
        val b = Bytes(200, little = true)
        b.text(0, "II"); b.u16(2, 42); b.u32(4, 8)
        b.ifd(8, listOf(intArrayOf(0x0112, SHORT, 1, 3)), next = 8)
        val info = RawPreview.inspect(ArraySource(b.data))
        assertTrue(info.previews.isEmpty())
        assertEquals(3, info.orientation)
    }

    @Test fun ファイルの外を指すプレビューは捨てる() {
        val b = Bytes(200, little = true)
        b.text(0, "II"); b.u16(2, 42); b.u32(4, 8)
        b.ifd(8, listOf(intArrayOf(0x0201, LONG, 1, 5000), intArrayOf(0x0202, LONG, 1, 1000)))
        assertTrue(RawPreview.inspect(ArraySource(b.data)).previews.isEmpty())
    }

    @Test fun RAWでないものや空は何も見つけない() {
        assertTrue(RawPreview.inspect(ArraySource(ByteArray(0))).previews.isEmpty())
        assertTrue(RawPreview.inspect(ArraySource(ByteArray(500) { it.toByte() })).previews.isEmpty())
        assertNull(RawPreview.largest(RawPreview.inspect(ArraySource(jpeg(100, 100, 100)))))
    }

    @Test fun 大きなAPP1の後ろのSOFも区切りを飛ばして読む() {
        val preview = jpeg(3000, 2000, 9000, app1 = 6000)
        val b = Bytes(12000, little = true)
        b.text(0, "II"); b.u16(2, 42); b.u32(4, 8)
        b.ifd(8, listOf(intArrayOf(0x0201, LONG, 1, 1000), intArrayOf(0x0202, LONG, 1, preview.size)))
        b.put(1000, preview)
        // 小さい区画で読む。**区画の境目をまたいでも同じ答え**になるか。
        val source = BlockCache(ArraySource(b.data), blockSize = 512)
        val largest = RawPreview.largest(RawPreview.inspect(source))!!
        assertEquals(3000, largest.width)
        assertArrayEquals(preview, RawPreview.bytesOf(source, largest))
    }

    @Test fun サムネイル用は長辺が足りる中で一番小さいもの() {
        val regions = listOf(
            JpegRegion(0, 10, 160, 120),
            JpegRegion(10, 10, 1616, 1080),
            JpegRegion(20, 10, 6000, 4000)
        )
        val info = RawInfo(regions, null, null)
        assertEquals(160, RawPreview.forThumb(info, minEdge = 160)!!.width)
        assertEquals(1616, RawPreview.forThumb(info, minEdge = 1000)!!.width)
        assertEquals(6000, RawPreview.largest(info)!!.width)
        // 足りるものが無ければ一番大きいもの。
        assertEquals(6000, RawPreview.forThumb(info, minEdge = 8000)!!.width)
    }
}
