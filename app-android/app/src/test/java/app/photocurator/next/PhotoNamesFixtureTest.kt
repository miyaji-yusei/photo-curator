package app.photocurator.next

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/**
 * R1: `core/tests/fixtures/photo-names.json`（PC の `tests.rs`・Web の `tests/photoNames.test.ts` も読む）を、
 * Android の NAS 走査の規則（[Smb] の isPhoto と隠し名の除外・[RawFiles.skipPairedRaw]）に通す。
 * 食い違いは表の `known_differences.android`。
 *
 * [Smb] の isPhoto は private で、フォルダのたどり（網）の中から呼ばれる。ここではリフレクションで
 * 呼び、「. で始まる名前（フォルダも）を除く」は Smb.kt の走査と同じ判定をこの試験で再現する。
 * **端末のアルバム（MediaStore）は MIME で選ぶので、この試験の対象外**。
 */
class PhotoNamesFixtureTest {
    private val fixture: JSONObject = run {
        // Gradle の単体試験の作業ディレクトリは app-android/app。
        val file = File("../../core/tests/fixtures/photo-names.json")
        assertTrue("フィクスチャが無い: ${file.absolutePath}", file.exists())
        JSONObject(file.readText(Charsets.UTF_8))
    }

    private fun strings(array: JSONArray): List<String> = List(array.length()) { array.getString(it) }

    private val isPhoto = Smb::class.java.getDeclaredMethod("isPhoto", String::class.java)
        .apply { isAccessible = true }

    private fun isPhoto(name: String): Boolean = isPhoto.invoke(Smb, name) as Boolean

    /** NAS の走査の候補: どの段も . で始まらず、拡張子が対象。 */
    private fun candidates(files: List<String>, pairRaw: Boolean): List<String> {
        val listed = files.filter { path ->
            path.split('/').none { it.startsWith(".") } && isPhoto(path.substringAfterLast('/'))
        }
        return RawFiles.skipPairedRaw(listed, pairRaw) { it }
    }

    private fun expected(case: JSONObject, key: String): List<String> {
        val difference = case.optJSONObject("known_differences")?.optJSONObject("android")
        val array = difference?.optJSONArray(key) ?: case.getJSONArray(key)
        return strings(array).sorted()
    }

    private fun cases(): List<JSONObject> {
        val array = fixture.getJSONArray("cases")
        return List(array.length()) { array.getJSONObject(it) }
    }

    @Test fun 表に十件以上のケースがある() {
        assertTrue(cases().size >= 10)
    }

    @Test fun 表のとおりに候補を選ぶ() {
        for (case in cases()) {
            val id = case.getString("id")
            val files = strings(case.getJSONArray("files"))
            for ((pairRaw, key) in listOf(true to "pair_on", false to "pair_off")) {
                assertEquals("$id / $key", expected(case, key), candidates(files, pairRaw).sorted())
            }
        }
    }

    @Test fun knownDifferencesは既定と本当に違う() {
        for (case in cases()) {
            val differences = case.optJSONObject("known_differences") ?: continue
            for (implementation in differences.keys()) {
                assertTrue(implementation in listOf("pc", "web", "android"))
                val difference = differences.getJSONObject(implementation)
                assertTrue(difference.optString("reason").isNotEmpty())
                for (key in listOf("pair_on", "pair_off")) {
                    if (!difference.has(key)) continue
                    assertNotEquals(
                        "${case.getString("id")} $implementation $key",
                        strings(case.getJSONArray(key)).sorted(),
                        strings(difference.getJSONArray(key)).sorted()
                    )
                }
            }
        }
    }
}
