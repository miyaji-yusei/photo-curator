package app.photocurator.next

import java.io.File
import java.util.concurrent.atomic.AtomicLong

private val temporaryCounter = AtomicLong()

/**
 * 途中まで書いて壊れたファイルを残さない。**同じフォルダの `.writing` に書いてから置き換える。**
 *
 * 11 か所に同じ形が散らばっていた（設計 09 章 §5 #4）。中身の書き方（テキスト・
 * バイト列・Bitmap の圧縮）だけが違うので、そこだけ [write] で渡す。
 *
 * **一時ファイルの名前は呼ぶたびに別にする。** 固定名だと、同じ先へ同時に書く 2 本が
 * 同じ一時ファイルを奪い合って、内容が混ざる・置き換えに失敗する。
 */
/** 端末のファイルを読んだ結果。**「無い」と「読めなかった」を分ける**（D2）。 */
sealed interface Stored<out T> {
    /** 読めた。ファイルが無ければ、呼ぶ側が決めた「無いとき」の値。 */
    data class Ok<T>(val value: T) : Stored<T>
    /** あるのに読めなかった。[keptAs] は控えを残した先（残せなければ null）。 */
    data class Broken(val reason: String, val keptAs: File?) : Stored<Nothing>
}

/**
 * 読む。**無ければ [missing]、あるのに読めなければ [Stored.Broken]。**
 * 読めなかったファイルは、元のファイルを残したまま `<名前>.broken-<時刻>.json` に写しを残す
 * （あとで上書きされても、元の中身を失わない）。同じ中身の写しが既にあれば増やさない。
 */
fun <T> File.readStored(missing: T, now: Long = System.currentTimeMillis(), parse: (String) -> T): Stored<T> {
    if (!exists()) return Stored.Ok(missing)
    val bytes = try {
        readBytes()
    } catch (error: Exception) {
        // 読み込みそのものの失敗（一時的なものも含む）。写しは作れない。
        return Stored.Broken(error.message ?: error.javaClass.simpleName, null)
    }
    return try {
        Stored.Ok(parse(String(bytes, Charsets.UTF_8)))
    } catch (error: Exception) {
        Stored.Broken(error.message ?: error.javaClass.simpleName, keepBroken(bytes, now))
    }
}

/** 読めなかった中身の写しを残す。**元のファイルは動かさない。** 残せなければ null。 */
private fun File.keepBroken(bytes: ByteArray, now: Long): File? = try {
    val base = name.removeSuffix(".json")
    val existing = parentFile?.listFiles { file ->
        file.name.startsWith("$base.broken-") && file.name.endsWith(".json")
    }.orEmpty()
    existing.firstOrNull { it.length() == bytes.size.toLong() && it.readBytes().contentEquals(bytes) }
        ?: run {
            val stamp = java.text.SimpleDateFormat("yyyyMMddHHmmss", java.util.Locale.ROOT)
                .apply { timeZone = java.util.TimeZone.getTimeZone("UTC") }
                .format(java.util.Date(now))
            var kept = File(parentFile, "$base.broken-$stamp.json")
            var n = 2
            while (kept.exists()) kept = File(parentFile, "$base.broken-$stamp-${n++}.json")
            kept.writeAtomically { it.writeBytes(bytes) }
            kept
        }
} catch (error: Exception) {
    null
}

fun File.writeAtomically(write: (File) -> Unit) {
    val temporary = File(parentFile, "$name.${temporaryCounter.incrementAndGet()}.writing")
    try {
        write(temporary)
        // 先にあっても置き換わる移動を使う（File.renameTo は環境によって先があると失敗する）。
        val moved = try {
            java.nio.file.Files.move(
                temporary.toPath(), toPath(), java.nio.file.StandardCopyOption.ATOMIC_MOVE
            )
            true
        } catch (error: Exception) {
            false
        }
        if (!moved && !temporary.renameTo(this)) {
            temporary.copyTo(this, overwrite = true)
        }
    } finally {
        // 置き換えに成功していれば無い。失敗・例外のときの残りを片付ける。
        temporary.delete()
    }
}
