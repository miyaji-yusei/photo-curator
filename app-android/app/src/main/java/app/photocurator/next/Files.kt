package app.photocurator.next

import java.io.File

/**
 * 途中まで書いて壊れたファイルを残さない。**同じフォルダの `.writing` に書いてから置き換える。**
 *
 * 11 か所に同じ形が散らばっていた（設計 09 章 §5 #4）。中身の書き方（テキスト・
 * バイト列・Bitmap の圧縮）だけが違うので、そこだけ [write] で渡す。
 */
fun File.writeAtomically(write: (File) -> Unit) {
    val temporary = File(parentFile, "$name.writing")
    write(temporary)
    if (!temporary.renameTo(this)) {
        temporary.copyTo(this, overwrite = true)
        temporary.delete()
    }
}
