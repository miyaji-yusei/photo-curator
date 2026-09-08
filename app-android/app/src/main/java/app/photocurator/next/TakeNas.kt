package app.photocurator.next

import android.content.ContentValues
import android.content.Context
import android.provider.MediaStore
import android.util.Log
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * NAS の写真を取り出す。**原本は動かさない。増やすだけ。**
 *
 * 端末の写真と違い、NAS の原本は「移す」ことをしない。移してしまうと、
 * 別の端末や PC から見たときにフォルダの中身が変わっていて、何が起きたのか
 * 説明できない。**取り出す＝コピー**に統一する。
 *
 * どちらも原本 1 枚 6MB を網から読むので時間がかかる。進みを数で出し、
 * 途中で失敗しても**どこまで済んだか**を残す。
 */
object TakeNas {
    private const val TAG = "TakeNas"

    /** SMB の区切り。 */
    private const val SEPARATOR = "\\"

    /** 何枚できて、何枚だめだったか。**「失敗」と「やめた」を混ぜない。** */
    data class Done(val done: Int, val failed: Int, val reason: String?) {
        fun describe(what: String): String = when {
            done > 0 && failed == 0 -> "$done 枚を$what"
            done > 0 -> "$done 枚を$what。$failed 枚はできませんでした（${reason ?: "理由不明"}）"
            else -> "できませんでした（${reason ?: "理由不明"}）"
        }
    }

    private suspend fun credentials(context: Context, project: Project): Pair<Nas, String>? {
        val nasId = project.source.key.substringBefore("|")
        val nas = NasStore.all(context).firstOrNull { it.id == nasId } ?: return null
        val password = Session.password(context, nas) ?: return null
        return nas to password
    }

    /**
     * 端末のギャラリーへ保存する。**NAS からコピーするだけ。**
     *
     * 入れ先は `Pictures/<フォルダ名>/`。端末のアルバムとして見えるので、
     * ほかのアプリからも扱える。同じ名前があっても上書きしない（MediaStore が
     * 名前を変える）。
     */
    suspend fun saveToGallery(
        context: Context,
        project: Project,
        photos: List<Photo>,
        album: String,
        onProgress: (Int, Int) -> Unit
    ): Done = withContext(Dispatchers.IO) {
        val (nas, password) = credentials(context, project)
            ?: return@withContext Done(0, photos.size, "NAS のパスワードが要ります")
        var done = 0
        var failed = 0
        var reason: String? = null
        // フォルダ名に使えない字は落とす。**MediaStore は黙って失敗する。**
        val folder = album.filter { it.isLetterOrDigit() || it == '_' || it == '-' || it == ' ' }
            .trim().ifBlank { "PhotoCurator" }

        val answer = Smb.reading(nas, password) { reader ->
            for (photo in photos) {
                val path = photo.smb?.path
                if (path == null) { failed += 1; continue }
                try {
                    // **1 枚ずつ読んで、1 枚ずつ書く。** まとめて持つと 6MB × 枚数の
                    // メモリを食う。途中で止まっても、書けた分は端末に残る。
                    val bytes = reader.whole(path)
                    if (bytes == null) {
                        failed += 1
                        if (reason == null) reason = "${photo.name} を読めませんでした"
                        continue
                    }
                    val values = ContentValues().apply {
                        put(MediaStore.MediaColumns.DISPLAY_NAME, photo.name)
                        put(MediaStore.MediaColumns.MIME_TYPE, mimeOf(photo.name))
                        put(MediaStore.MediaColumns.RELATIVE_PATH, "Pictures/$folder/")
                        put(MediaStore.MediaColumns.IS_PENDING, 1)
                    }
                    val uri = context.contentResolver
                        .insert(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, values)
                    if (uri == null) {
                        failed += 1
                        if (reason == null) reason = "端末に書き込めませんでした"
                        continue
                    }
                    context.contentResolver.openOutputStream(uri)?.use { it.write(bytes) }
                    // **書き終えてから見せる。** 途中の中途半端なファイルを
                    // ギャラリーに出さないため。
                    context.contentResolver.update(
                        uri,
                        ContentValues().apply { put(MediaStore.MediaColumns.IS_PENDING, 0) },
                        null, null
                    )
                    done += 1
                } catch (error: Exception) {
                    failed += 1
                    Log.w(TAG, "保存できなかった: ${photo.name}", error)
                    if (reason == null) reason = error.message?.take(80) ?: "書き込みに失敗"
                }
                onProgress(done + failed, photos.size)
            }
        }
        if (answer is SmbResult.Failed) {
            return@withContext Done(done, photos.size - done, answer.reason)
        }
        Done(done, failed, reason)
    }

    /**
     * NAS の中で星ごとのフォルダに分ける。**コピー。原本は残す。**
     *
     * 行き先は写真のあるフォルダの直下に `star-5 / star-4 …`。
     * 移動にしないのは、別の端末から見て「写真が消えた」ように見えるため。
     */
    suspend fun sortOnNas(
        context: Context,
        project: Project,
        photos: List<Photo>,
        ratings: Map<String, Int>,
        onProgress: (Int, Int) -> Unit
    ): Done = withContext(Dispatchers.IO) {
        val (nas, password) = credentials(context, project)
            ?: return@withContext Done(0, photos.size, "NAS のパスワードが要ります")
        val root = project.source.key.substringAfter("|")
        var done = 0
        var failed = 0
        var reason: String? = null

        for (photo in photos) {
            val path = photo.smb?.path
            if (path == null) { failed += 1; continue }
            val star = ratings[photo.relativePath] ?: 0
            val destination = root + SEPARATOR + "star-" + star + SEPARATOR + photo.name
            try {
                // 読んで、書く。**同じ接続で往復させない**（読みと書きで別の
                // 接続になるが、1 枚 6MB なので接続の張り直しは誤差）。
                val bytes = when (val got = Smb.readIfExists(nas, password, path)) {
                    is SmbResult.Failed -> {
                        failed += 1
                        if (reason == null) reason = got.reason
                        onProgress(done + failed, photos.size)
                        continue
                    }
                    is SmbResult.Ok -> got.value
                }
                if (bytes == null) {
                    failed += 1
                    if (reason == null) reason = "${photo.name} が見つかりません"
                    onProgress(done + failed, photos.size)
                    continue
                }
                when (val wrote = Smb.write(nas, password, destination, bytes)) {
                    is SmbResult.Failed -> {
                        failed += 1
                        if (reason == null) reason = wrote.reason
                    }
                    is SmbResult.Ok -> done += 1
                }
            } catch (error: Exception) {
                failed += 1
                Log.w(TAG, "分けられなかった: ${photo.name}", error)
                if (reason == null) reason = error.message?.take(80) ?: "コピーに失敗"
            }
            onProgress(done + failed, photos.size)
        }
        Done(done, failed, reason)
    }

    private fun mimeOf(name: String): String =
        when (name.substringAfterLast('.', "").lowercase()) {
            "png" -> "image/png"
            "webp" -> "image/webp"
            else -> "image/jpeg"
        }
}
