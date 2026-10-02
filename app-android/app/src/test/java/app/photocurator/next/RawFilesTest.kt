package app.photocurator.next

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** U49: PC（U46 の `skip_paired_raw`）・Web（`skipPairedRaw`）と同じ規則か。 */
class RawFilesTest {
    private fun kept(paths: List<String>, enabled: Boolean = true) =
        RawFiles.skipPairedRaw(paths, enabled) { it }

    @Test fun 同名のJPEGがあるRAWだけ外す() {
        val paths = listOf(
            "d/IMG_1.JPG", "d/IMG_1.CR2",
            "d/IMG_2.CR2",
            "d/IMG_3.jpeg", "d/IMG_3.NEF"
        )
        assertEquals(listOf("d/IMG_1.JPG", "d/IMG_2.CR2", "d/IMG_3.jpeg"), kept(paths))
    }

    @Test fun PCのテストと同じ並びになる() {
        val paths = listOf("d/a.jpg", "d/A.cr3", "d/b.cr3", "e/a.arw", "d/c.heif", "d/c.jpeg")
        assertEquals(listOf("d/a.jpg", "d/b.cr3", "e/a.arw", "d/c.heif", "d/c.jpeg"), kept(paths))
    }

    @Test fun 別フォルダの同名は外さない() {
        assertEquals(listOf("raw/a.cr2", "jpg/a.jpg"), kept(listOf("raw/a.cr2", "jpg/a.jpg")))
    }

    @Test fun HEICやPNGやWebPとの組は外さない() {
        val paths = listOf("d/a.heic", "d/a.dng", "d/b.png", "d/b.arw", "d/c.webp", "d/c.raf")
        assertEquals(paths, kept(paths))
    }

    @Test fun RAW同士は外さない() {
        val paths = listOf("d/a.cr2", "d/a.dng")
        assertEquals(paths, kept(paths))
    }

    @Test fun 大文字小文字を区別しない() {
        assertEquals(listOf("d/Img_9.jPg"), kept(listOf("d/Img_9.jPg", "d/IMG_9.Rw2")))
    }

    @Test fun 十種類のRAWすべてが対象() {
        for (ext in listOf("cr2", "cr3", "nef", "arw", "dng", "raf", "orf", "rw2", "pef", "srw")) {
            assertEquals(ext, listOf("d/x.jpg"), kept(listOf("d/x.jpg", "d/x.$ext")))
        }
    }

    @Test fun オフなら全部残す() {
        val paths = listOf("d/a.jpg", "d/a.cr2", "d/b.cr2")
        assertEquals(paths, kept(paths, enabled = false))
    }

    @Test fun 区切りが逆スラッシュでもフォルダで比べる() {
        assertEquals(listOf("2024\\a.jpg"), kept(listOf("2024\\a.jpg", "2024\\a.cr2")))
        assertEquals(listOf("2024\\a.jpg", "2025\\a.cr2"), kept(listOf("2024\\a.jpg", "2025\\a.cr2")))
    }

    @Test fun 名前の途中の点は名前の一部() {
        assertEquals(listOf("d/a.b.jpg"), kept(listOf("d/a.b.jpg", "d/a.b.cr2")))
        assertEquals(listOf("d/a.jpg", "d/a.b.cr2"), kept(listOf("d/a.jpg", "d/a.b.cr2")))
    }

    @Test fun RAWかどうかを拡張子で見る() {
        assertTrue(RawFiles.isRaw("IMG_0001.CR2"))
        assertTrue(RawFiles.isRaw("dir/x.dng"))
        assertFalse(RawFiles.isRaw("x.jpg"))
        assertFalse(RawFiles.isRaw("x.heic"))
        assertFalse(RawFiles.isRaw(".cr2"))
        assertFalse(RawFiles.isRaw("cr2"))
    }
}
