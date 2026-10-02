package app.photocurator.next

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.nio.file.Files

/** 端末のファイルの「無い」と「読めなかった」（U44 の D2）。 */
class StoredFileTest {
    private fun dir(): File = Files.createTempDirectory("stored").toFile()

    private val parse: (String) -> Int = { text ->
        require(text.startsWith("v1:")) { "形が違う" }
        text.removePrefix("v1:").toInt()
    }

    @Test fun 無ければ無いときの値() {
        val file = File(dir(), "session-p.json")
        assertEquals(Stored.Ok(-1), file.readStored(-1, parse = parse))
    }

    @Test fun 読めればその値() {
        val file = File(dir(), "session-p.json").apply { writeText("v1:42") }
        assertEquals(Stored.Ok(42), file.readStored(-1, parse = parse))
    }

    @Test fun 端末の書きかけはそのプロジェクトの古いものだけ片付ける() {
        val folder = dir()
        val now = 1_790_000_000_000L
        val old = now - 2 * 60 * 60 * 1000L
        fun make(name: String, at: Long) = File(folder, name).apply { writeText("x"); setLastModified(at) }
        make("session-p.json.12.writing", old)
        make("overrides-p.json.13.writing", old)
        make("session-p.json.14.writing", now - 1_000)
        make("session-q.json.15.writing", old)
        make("session-p.json", old)
        make("thumb.jpg.16.writing", old)

        val gone = cleanWriting(folder, listOf("session-p.json.", "overrides-p.json."), now)

        assertEquals(setOf("session-p.json.12.writing", "overrides-p.json.13.writing"), gone.map { it.name }.toSet())
        assertEquals(
            setOf("session-p.json.14.writing", "session-q.json.15.writing", "session-p.json", "thumb.jpg.16.writing"),
            folder.listFiles()!!.map { it.name }.toSet()
        )
    }

    @Test fun 読めなければ空にせず元のファイルを残し写しを1つだけ作る() {
        val folder = dir()
        val file = File(folder, "session-p.json").apply { writeText("v2:{\"new\":true}") }

        val first = file.readStored(-1, now = 1_790_000_000_000L, parse = parse)
        assertTrue("読めなかった: $first", first is Stored.Broken)
        val kept = (first as Stored.Broken).keptAs
        assertNotNull(kept)
        assertTrue(kept!!.name, kept.name.startsWith("session-p.broken-") && kept.name.endsWith(".json"))
        assertEquals("写しは元と同じ中身", "v2:{\"new\":true}", kept.readText())
        assertEquals("元のファイルはそのまま", "v2:{\"new\":true}", file.readText())

        // もう一度読んでも、同じ中身の写しは増やさない。
        val second = file.readStored(-1, now = 1_790_000_100_000L, parse = parse)
        assertTrue(second is Stored.Broken)
        assertEquals(kept, (second as Stored.Broken).keptAs)
        assertEquals(1, folder.listFiles()!!.count { it.name.contains(".broken-") })

        // 中身が変わってまた読めなければ、別の写しを足す（前の写しは消さない）。
        file.writeText("v3:other")
        val third = file.readStored(-1, now = 1_790_000_200_000L, parse = parse)
        assertTrue(third is Stored.Broken)
        assertEquals(2, folder.listFiles()!!.count { it.name.contains(".broken-") })
    }
}
