package app.photocurator.next

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * U50（レビュー A17 (a)）: projects.json の 1 件が読めないだけで、**一覧が空になり、次の保存で
 * 新しい 1 件だけに上書きされる**のを防ぐ。読めた分は返し、読めない分は中身のまま書き戻す。
 */
class ProjectsParseTest {
    private val good = """{"id":"p1","name":"旅行","created":1,"updated":5,
        "source":{"kind":"album","label":"Camera","key":"b1"}}"""
    private val good2 = """{"id":"p2","name":"NAS","created":2,"updated":9,
        "source":{"kind":"nas","label":"共有","key":"n1|photos"}}"""
    // 新しい版のアプリが書いた種類（この版は知らない）。
    private val future = """{"id":"p3","name":"未来","created":3,"updated":7,
        "source":{"kind":"gdrive","label":"Drive","key":"x"}}"""
    private val broken = """{"id":"p4","name":"欠け"}"""

    @Test fun 一件読めなくても残りは返す() {
        val parsed = Projects.parse("[$good,$future,$good2,$broken]")
        assertEquals(listOf("p2", "p1"), parsed.projects.map { it.id }) // 更新順
        assertEquals(2, parsed.unknown.size)
    }

    @Test fun 読めない分は書き戻すときに落とさない() {
        val parsed = Projects.parse("[$good,$future]")
        val written = Projects.serialize(parsed.projects, parsed.unknown)
        val again = Projects.parse(written)
        assertEquals(listOf("p1"), again.projects.map { it.id })
        assertEquals(1, again.unknown.size)
        assertTrue(again.unknown.single().contains("gdrive"))
    }

    @Test fun 全部読めれば今までと同じ() {
        val parsed = Projects.parse("[$good,$good2]")
        assertEquals(2, parsed.projects.size)
        assertTrue(parsed.unknown.isEmpty())
        val p1 = parsed.projects.first { it.id == "p1" }
        assertEquals(Source(SourceKind.Album, "Camera", "b1"), p1.source)
        assertEquals(1L, p1.createdAt)
    }
}
