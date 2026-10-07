package app.photocurator.next

import android.content.Context
import android.util.Log
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.File

/** 解析できなかった理由の種類（U58）。 */
enum class FailureKind {
    /** 読めなかった・打ち切り・取得の失敗など。**次に開いたときにもう一度試す。** */
    Transient,

    /** 全部読めたのに復号できない（壊れた・非対応の形式）。**原本が変わるまで試さない。** */
    Unsupported
}

/**
 * 解析できなかった 1 枚の記録。
 *
 * [size]・[version] は**そのときの原本と作り方の版**。非対応は、この 2 つが今と同じ間だけ
 * 「もう試さない」とする（大きさが変わった＝差し替わった、版が上がった＝作り方が変わった、で再試行）。
 */
data class Failure(
    val kind: FailureKind,
    val reason: String,
    val size: Long,
    val version: Int,
    val at: Long
)

/** 一覧に出す 1 行。 */
data class FailureRow(val relativePath: String, val name: String, val kind: FailureKind, val reason: String)

/**
 * 解析できなかった写真の控え（U58）。**どのファイルを・なぜ・いつの原本で**。
 *
 * ファイル名は [CacheName.of]（SHA-256）で決める。`Fingerprints` のように英数字以外を `_` に
 * 潰すだけだと、同じ長さの日本語のフォルダ 2 つが同じファイルを使ってしまう（B2）。
 * 読めなければ空（＝全部試す。安全側）。
 */
object Failures {
    private const val TAG = "Failures"

    /** 出所の鍵から決まる名前。**SHA-256 なので、似た名前のフォルダでも混ざらない**（B2 を持ち込まない）。 */
    internal fun fileName(sourceKey: String) = "failures-${CacheName.of(sourceKey)}.json"

    internal fun file(context: Context, sourceKey: String) = File(context.filesDir, fileName(sourceKey))

    /** 非対応と確定しているか。**原本（大きさ）と作り方の版が控えのときと同じ間だけ。** */
    fun isUnsupported(photo: Photo, failure: Failure?): Boolean =
        failure != null && failure.kind == FailureKind.Unsupported &&
            failure.size == photo.size && failure.version == Analyse.VERSION

    /** 顔ぶれのうち、非対応と確定している写真の数。 */
    fun unsupportedCount(photos: List<Photo>, failures: Map<String, Failure>): Int =
        if (failures.isEmpty()) 0 else photos.count { isUnsupported(it, failures[it.relativePath]) }

    /** 選別の対象（準備の分母）の枚数。 */
    fun workableCount(photos: List<Photo>, failures: Map<String, Failure>): Int =
        photos.size - unsupportedCount(photos, failures)

    /**
     * 一覧に出す行。**相対パス順。** 顔ぶれに無い写真・もう試し直す対象の古い非対応
     * （原本が変わった）は出さない。
     */
    fun rows(photos: List<Photo>, failures: Map<String, Failure>): List<FailureRow> =
        photos.mapNotNull { photo ->
            val failure = failures[photo.relativePath] ?: return@mapNotNull null
            if (failure.kind == FailureKind.Unsupported && !isUnsupported(photo, failure)) return@mapNotNull null
            FailureRow(photo.relativePath, photo.name, failure.kind, failure.reason)
        }.sortedBy { it.relativePath }

    /**
     * 読む。**1 件読めなくても残りは返す。** 全体が壊れていれば空（全部試し直す）。
     * 空にしても、試して駄目だった写真をもう一度試すだけで、選別の記録は壊れない。
     */
    internal fun parse(text: String): Map<String, Failure> = try {
        val root = org.json.JSONObject(text)
        val out = HashMap<String, Failure>(root.length())
        for (path in root.keys()) {
            try {
                val entry = root.getJSONObject(path)
                val kind = when (entry.getString("k")) {
                    "unsupported" -> FailureKind.Unsupported
                    "transient" -> FailureKind.Transient
                    else -> continue
                }
                out[path] = Failure(
                    kind = kind,
                    reason = entry.optString("r", ""),
                    size = entry.getLong("size"),
                    version = entry.getInt("v"),
                    at = entry.optLong("at", 0L)
                )
            } catch (error: Exception) {
                // 1 件だけ読めない。残りは使う（その 1 枚はもう一度試すことになる）。
            }
        }
        out
    } catch (error: Exception) {
        emptyMap()
    }

    internal fun serialize(failures: Map<String, Failure>): String {
        val root = org.json.JSONObject()
        for ((path, failure) in failures) {
            root.put(
                path,
                org.json.JSONObject()
                    .put("k", if (failure.kind == FailureKind.Unsupported) "unsupported" else "transient")
                    .put("r", failure.reason)
                    .put("size", failure.size)
                    .put("v", failure.version)
                    .put("at", failure.at)
            )
        }
        return root.toString()
    }

    suspend fun load(context: Context, sourceKey: String): Map<String, Failure> =
        withContext(Dispatchers.IO) {
            val target = file(context, sourceKey)
            if (!target.exists()) return@withContext emptyMap()
            try {
                parse(target.readText())
            } catch (error: Exception) {
                Log.w(TAG, "読めなかった: $sourceKey", error)
                emptyMap()
            }
        }

    suspend fun save(context: Context, sourceKey: String, failures: Map<String, Failure>) =
        Persist.latest("failures:$sourceKey") {
            try {
                val target = file(context, sourceKey)
                if (failures.isEmpty()) target.delete()
                else target.writeAtomically { it.writeText(serialize(failures)) }
            } catch (error: Exception) {
                Log.w(TAG, "保存できなかった: $sourceKey", error)
            }
        }
}
