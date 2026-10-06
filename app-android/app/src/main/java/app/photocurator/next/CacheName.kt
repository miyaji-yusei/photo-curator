package app.photocurator.next

import android.content.Context
import java.io.File
import java.security.MessageDigest

/**
 * 絵のキャッシュ（`thumbs/`・`renders/`）のファイル名に使う、道筋の短い名前。
 *
 * **32 ビットの `hashCode()` は使わない。** 2,000 枚で 0.05%、20,000 枚で約 4.5% の確率で
 * 別の 2 枚が同じ名前になり、選別画面に**別の写真の絵**が出る。SHA-256 の先頭 128 ビット
 * （16 進で 32 文字）なら、現実の枚数で衝突しない。
 *
 * 旧名（`hashCode()` の 16 進 8 桁以下）とは桁数で見分けられる。旧名のファイルは読まない
 * （作り直しになる）。残りは [dropLegacy] が一度だけ片付ける。
 */
object CacheName {
    private const val DIGITS = 32
    private val CURRENT = Regex("^.+_[0-9a-f]{" + DIGITS + "}(_[0-9]+)?[.]jpg$")

    /** 道筋から決まる名前。**同じ道筋なら同じ値。** */
    fun of(path: String): String {
        val digest = MessageDigest.getInstance("SHA-256").digest(path.toByteArray(Charsets.UTF_8))
        val out = StringBuilder(DIGITS)
        for (at in 0 until DIGITS / 2) out.append("%02x".format(digest[at].toInt() and 0xff))
        return out.toString()
    }

    /** 新しい名前の絵か。 */
    fun isCurrent(fileName: String): Boolean = CURRENT.matches(fileName)

    private val RENDER = Regex("^(.+)_[0-9a-f]{" + DIGITS + "}_([0-9]+)[.]jpg$")

    /** 表示用画像の名前から（置き場の id, 大きさ）を取り出す。そうでなければ null。 */
    fun parseRender(fileName: String): Pair<String, Int>? {
        val match = RENDER.matchEntire(fileName) ?: return null
        val edge = match.groupValues[2].toIntOrNull() ?: return null
        return match.groupValues[1] to edge
    }

    /** 表示用画像の名前から（置き場の id, 道筋の名前 [of], 大きさ）を取り出す。そうでなければ null。 */
    fun parseRenderParts(fileName: String): Triple<String, String, Int>? {
        val match = RENDER_PARTS.matchEntire(fileName) ?: return null
        val edge = match.groupValues[3].toIntOrNull() ?: return null
        return Triple(match.groupValues[1], match.groupValues[2], edge)
    }

    private val RENDER_PARTS = Regex("^(.+)_([0-9a-f]{" + DIGITS + "})_([0-9]+)[.]jpg$")

    /** 新しい名前の絵で、大きさが [edge] のものか。 */
    fun isRender(fileName: String, cacheId: String, edge: Int): Boolean =
        Regex("^" + Regex.escape(cacheId) + "_[0-9a-f]{" + DIGITS + "}_" + edge + "[.]jpg$").matches(fileName)

    /**
     * 旧名の絵（もう読まれない）を消す。**一度だけ。** 容量の無駄になるので。
     * 消すのは `.jpg` で新しい名前でないものだけ。
     */
    fun dropLegacy(context: Context) {
        val prefs = context.getSharedPreferences("prefs", Context.MODE_PRIVATE)
        if (prefs.getBoolean("cache_names_v2", false)) return
        for (name in listOf("thumbs", "renders")) {
            File(context.filesDir, name).listFiles { file ->
                file.name.endsWith(".jpg") && !isCurrent(file.name)
            }?.forEach { it.delete() }
        }
        prefs.edit().putBoolean("cache_names_v2", true).apply()
    }
}
