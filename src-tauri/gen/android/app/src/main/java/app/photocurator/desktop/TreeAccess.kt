package app.photocurator.desktop

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.DocumentsContract
import org.json.JSONArray
import org.json.JSONObject
import java.io.InputStream

/**
 * SAF（Storage Access Framework）のフォルダから写真を読む口。
 *
 * ## なぜこれがあるか
 *
 * NAS へは smbj で直接繋ぐのが第一の道（`SmbAccess`）だが、実機で繋がらない
 * ことがありうる（SMB のバージョン、認証方式、端末の制約）。そのときの代替。
 *
 * **NAS のベンダー製アプリ（WebAccess A など）が DocumentsProvider として
 * 登録されていれば、SAF のフォルダ選択にその NAS が現れる。** そこを選べば、
 * SMB を自分で話さずに NAS の写真へ届く。端末内のフォルダも同じ仕組みで扱える。
 *
 * ## 制約
 *
 * - **部分読みが効くかは提供元しだい。** `openInputStream` を途中で止めれば
 *   そこまでしか転送しない実装が多いが、保証はない。効かなければ 1 枚 6.7MB
 *   が流れる（`SmbAccess` なら offset/length を渡せるので確実に効く）
 * - フォルダを選ぶのに Activity が要るので、`MainActivity` が受け口を持つ
 */
object TreeAccess {
    private const val EXTENSIONS_SEPARATOR = ","
    private val extensions = setOf("jpg", "jpeg", "png", "webp")

    @Volatile
    private var context: Context? = null

    /** 選択の結果を受け取る場所。`MainActivity` が入れる。 */
    @Volatile
    private var lastPickedTree: String = ""

    @JvmStatic
    fun attach(context: Context) {
        this.context = context.applicationContext
    }

    private fun requireContext(): Context =
        context ?: throw IllegalStateException("TreeAccess にコンテキストが渡されていません。")

    /** `MainActivity` がフォルダ選択の結果を渡す。権限は永続化済み。 */
    @JvmStatic
    fun setPickedTree(uri: String) {
        lastPickedTree = uri
    }

    /**
     * 直近に選ばれたフォルダ。**選択そのものは非同期**なので、
     * Rust は「選択を開く」→「あとで取りに来る」の 2 段で使う。
     */
    @JvmStatic
    fun takePickedTree(): String {
        val picked = lastPickedTree
        lastPickedTree = ""
        return picked
    }

    /** 既に権限を持っているフォルダ。アプリを再起動しても残る。 */
    @JvmStatic
    fun listGrantedTrees(): String {
        val array = JSONArray()
        for (permission in requireContext().contentResolver.persistedUriPermissions) {
            if (!permission.isReadPermission) continue
            val uri = permission.uri
            array.put(
                JSONObject()
                    .put("path", uri.toString())
                    .put("name", displayNameOf(uri))
            )
        }
        return array.toString()
    }

    private fun displayNameOf(treeUri: Uri): String {
        // ツリーの URI から素直に名前を引けないので、末尾を人が読める形にする。
        val id = runCatching { DocumentsContract.getTreeDocumentId(treeUri) }.getOrNull()
            ?: return treeUri.lastPathSegment ?: treeUri.toString()
        return id.substringAfterLast(':').substringAfterLast('/').ifEmpty { id }
    }

    /**
     * ツリー以下の写真を**再帰的に**集める。デスクトップの WalkDir と同じ範囲。
     * 返す `uri` はそのままドキュメントの URI で、`PhotoAccess.readBytes` と
     * 同じ経路で読める（どちらも `content://`）。
     */
    @JvmStatic
    fun listPhotos(treeUri: String): String {
        val resolver = requireContext().contentResolver
        val root = Uri.parse(treeUri)
        val rootId = runCatching { DocumentsContract.getTreeDocumentId(root) }.getOrNull()
            ?: return "[]"
        val array = JSONArray()
        val pending = ArrayDeque<Pair<String, String>>()
        pending.add(rootId to "")

        while (pending.isNotEmpty()) {
            val (documentId, prefix) = pending.removeFirst()
            val children = DocumentsContract.buildChildDocumentsUriUsingTree(root, documentId)
            val projection = arrayOf(
                DocumentsContract.Document.COLUMN_DOCUMENT_ID,
                DocumentsContract.Document.COLUMN_DISPLAY_NAME,
                DocumentsContract.Document.COLUMN_MIME_TYPE,
                DocumentsContract.Document.COLUMN_SIZE,
                DocumentsContract.Document.COLUMN_LAST_MODIFIED
            )
            val cursor = runCatching {
                resolver.query(children, projection, null, null, null)
            }.getOrNull() ?: continue
            cursor.use {
                while (it.moveToNext()) {
                    val childId = it.getString(0) ?: continue
                    val name = it.getString(1) ?: continue
                    val mime = it.getString(2) ?: ""
                    val relative = if (prefix.isEmpty()) name else "$prefix/$name"
                    if (mime == DocumentsContract.Document.MIME_TYPE_DIR) {
                        pending.add(childId to relative)
                        continue
                    }
                    val extension = name.substringAfterLast('.', "").lowercase()
                    if (extension !in extensions) continue
                    val documentUri = DocumentsContract.buildDocumentUriUsingTree(root, childId)
                    array.put(
                        JSONObject()
                            .put("uri", documentUri.toString())
                            .put("name", name)
                            .put("relativePath", relative)
                            .put("size", it.getLong(3))
                            // SAF の LAST_MODIFIED は既にミリ秒。
                            .put("modifiedAt", it.getLong(4))
                    )
                }
            }
        }
        return array.toString()
    }

    /** 1 枚ぶんの mtime と size。 */
    @JvmStatic
    fun stat(uri: String): String? {
        val projection = arrayOf(
            DocumentsContract.Document.COLUMN_SIZE,
            DocumentsContract.Document.COLUMN_LAST_MODIFIED
        )
        return runCatching {
            requireContext().contentResolver.query(Uri.parse(uri), projection, null, null, null)
                ?.use { cursor ->
                    if (!cursor.moveToFirst()) return@runCatching null
                    JSONObject()
                        .put("size", cursor.getLong(0))
                        .put("modifiedAt", cursor.getLong(1))
                        .toString()
                }
        }.getOrNull()
    }

    /**
     * バイト列。`PhotoAccess.readBytes` と同じ形。
     *
     * **部分読みが効くかは提供元しだい。** 途中で読むのをやめれば、そこまでしか
     * 転送しない実装が多いが、保証はない。`SmbAccess` のように offset/length を
     * プロトコルへ渡せるわけではない。
     */
    @JvmStatic
    fun readBytes(uri: String, offset: Long, length: Int): ByteArray? {
        return try {
            requireContext().contentResolver.openInputStream(Uri.parse(uri))?.use { stream ->
                var remaining = offset
                while (remaining > 0) {
                    val skipped = stream.skip(remaining)
                    if (skipped <= 0) break
                    remaining -= skipped
                }
                if (length <= 0) stream.readBytes() else readAtMost(stream, length)
            }
        } catch (error: Exception) {
            null
        }
    }

    private fun readAtMost(stream: InputStream, length: Int): ByteArray {
        val buffer = ByteArray(length)
        var filled = 0
        while (filled < length) {
            val read = stream.read(buffer, filled, length - filled)
            if (read < 0) break
            filled += read
        }
        return if (filled == length) buffer else buffer.copyOf(filled)
    }

    /** フォルダ選択の Intent。`MainActivity` が投げる。 */
    @JvmStatic
    fun pickIntent(): Intent =
        Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
            addFlags(
                Intent.FLAG_GRANT_READ_URI_PERMISSION or
                    Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
            )
        }
}
