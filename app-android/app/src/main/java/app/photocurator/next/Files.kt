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
