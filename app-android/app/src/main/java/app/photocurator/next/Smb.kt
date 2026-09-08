package app.photocurator.next

import android.util.Log
import com.hierynomus.msdtyp.AccessMask
import com.hierynomus.mssmb2.SMB2CreateDisposition
import com.hierynomus.mssmb2.SMB2ShareAccess
import com.hierynomus.smbj.SMBClient
import com.hierynomus.smbj.auth.AuthenticationContext
import com.hierynomus.smbj.share.DiskShare
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.util.EnumSet

/** NAS の中のフォルダ 1 つ。 */
data class SmbFolder(val name: String, val path: String, val count: Int)

/** NAS の中の写真 1 枚。 */
data class SmbPhoto(
    val name: String,
    /** 共有の中での道筋。**プロジェクトの鍵になる。** */
    val path: String,
    val size: Long,
    val modifiedAt: Long
)

/** つないだ結果。**成功か、理由の付いた失敗か。** */
sealed interface SmbResult<out T> {
    data class Ok<T>(val value: T) : SmbResult<T>
    data class Failed(val reason: String) : SmbResult<Nothing>
}

/**
 * NAS（SMB）を読む。**読むだけ。** 書き込みの経路はここに置かない。
 *
 * 接続は毎回張って毎回閉じる。開きっぱなしにすると、Wi-Fi が切れたときに
 * 「繋がっているつもり」の状態が残り、次の操作が理由の分からない失敗になる。
 * 張り直しは速いので、**状態を持たない方が説明しやすい。**
 */
object Smb {
    private const val TAG = "Smb"

    /** このアプリが扱う形式。MediaStore 側と揃える。 */
    private val EXTENSIONS = setOf("jpg", "jpeg", "png", "webp")

    /**
     * 1 回だけつないで、中で仕事をして、必ず閉じる。
     *
     * 失敗は**理由を文にして返す**。「接続できません」だけでは、
     * Wi-Fi なのか共有名なのかパスワードなのか分からない。
     */
    private suspend fun <T> connect(
        nas: Nas,
        password: String,
        work: (DiskShare) -> T
    ): SmbResult<T> = withContext(Dispatchers.IO) {
        try {
            SMBClient().use { client ->
                client.connect(nas.host).use { connection ->
                    val session = connection.authenticate(
                        AuthenticationContext(nas.user, password.toCharArray(), null)
                    )
                    val share = session.connectShare(nas.share) as? DiskShare
                        ?: return@withContext SmbResult.Failed(
                            "「${nas.share}」は共有フォルダではありません"
                        )
                    share.use { SmbResult.Ok(work(it)) }
                }
            }
        } catch (error: Exception) {
            Log.w(TAG, "NAS につなげない: ${nas.host}/${nas.share}", error)
            SmbResult.Failed(describe(error))
        }
    }

    /** 例外を人の言葉にする。**次に何をすればいいかが分かる言い方で。** */
    private fun describe(error: Exception): String {
        val message = error.message.orEmpty()
        return when {
            message.contains("STATUS_LOGON_FAILURE", true) ->
                "ユーザー名かパスワードが違います"
            message.contains("STATUS_BAD_NETWORK_NAME", true) ->
                "共有名が見つかりません"
            message.contains("STATUS_ACCESS_DENIED", true) ->
                "このユーザーには読む権限がありません"
            error is java.net.UnknownHostException ->
                "ホスト名を解決できません。IP で試してください"
            error is java.net.SocketTimeoutException || message.contains("timed out", true) ->
                "応答がありません。同じ Wi-Fi につながっているか確認してください"
            error is java.net.ConnectException ->
                "接続を拒否されました。ホストとポートを確認してください"
            else -> message.take(80).ifEmpty { error.javaClass.simpleName }
        }
    }

    /** つながるかだけ試す。**設定画面の「接続を確認」。** */
    suspend fun check(nas: Nas, password: String): SmbResult<Int> =
        connect(nas, password) { share ->
            // root を 1 回読めれば、ホスト・共有・認証がすべて通っている。
            share.list("").count { !it.fileName.startsWith(".") }
        }

    /**
     * フォルダの一覧。**写真が入っているものだけ**を、枚数付きで返す。
     *
     * 空のフォルダを出しても選べないので、数えるついでに落とす。
     */
    suspend fun folders(nas: Nas, password: String, parent: String = ""): SmbResult<List<SmbFolder>> =
        connect(nas, password) { share ->
            val found = ArrayList<SmbFolder>()
            for (entry in share.list(parent)) {
                val name = entry.fileName
                if (name == "." || name == ".." || name.startsWith(".")) continue
                val path = if (parent.isEmpty()) name else "$parent\\$name"
                val isDirectory = try {
                    share.folderExists(path)
                } catch (error: Exception) {
                    false
                }
                if (!isDirectory) continue
                val count = try {
                    share.list(path).count { isPhoto(it.fileName) }
                } catch (error: Exception) {
                    // 数えられないフォルダは 0 にせず落とす。**嘘の数を出さない。**
                    Log.w(TAG, "数えられなかった: $path", error)
                    -1
                }
                if (count > 0) found += SmbFolder(name, path, count)
            }
            found.sortedBy { it.name }
        }

    /** あるフォルダの写真。**更新時刻の昇順**で返す。 */
    suspend fun photos(nas: Nas, password: String, folder: String): SmbResult<List<SmbPhoto>> =
        connect(nas, password) { share ->
            share.list(folder)
                .filter { isPhoto(it.fileName) }
                .map { entry ->
                    SmbPhoto(
                        name = entry.fileName,
                        path = if (folder.isEmpty()) entry.fileName else "$folder\\${entry.fileName}",
                        size = entry.endOfFile,
                        // SMB の時刻は 1601 年起点。Java の epoch へ直す。
                        modifiedAt = entry.lastWriteTime.toEpochMillis()
                    )
                }
                // **使う値そのもので並べる。** 同時刻は名前で決める（毎回同じ順）。
                .sortedWith(compareBy({ it.modifiedAt }, { it.name }))
        }

    /**
     * ファイルの先頭を読む。**全部は読まない。**
     *
     * 指紋と一覧のサムネイルには EXIF の縮小画像で足りる。原本 1 枚 6MB を
     * 網越しに引くと、2,000 枚で 12GB になる。前の版はこれで詰まっていた。
     */
    suspend fun head(nas: Nas, password: String, path: String, bytes: Int): SmbResult<ByteArray> =
        connect(nas, password) { share ->
            share.openFile(
                path,
                EnumSet.of(AccessMask.GENERIC_READ),
                null,
                SMB2ShareAccess.ALL,
                SMB2CreateDisposition.FILE_OPEN,
                null
            ).use { file ->
                file.inputStream.use { stream ->
                    val buffer = ByteArray(bytes)
                    var filled = 0
                    while (filled < bytes) {
                        val read = stream.read(buffer, filled, bytes - filled)
                        if (read <= 0) break
                        filled += read
                    }
                    buffer.copyOf(filled)
                }
            }
        }

    /** 原本を丸ごと読む。**拡大表示のときだけ。** */
    suspend fun whole(nas: Nas, password: String, path: String): SmbResult<ByteArray> =
        connect(nas, password) { share ->
            share.openFile(
                path,
                EnumSet.of(AccessMask.GENERIC_READ),
                null,
                SMB2ShareAccess.ALL,
                SMB2CreateDisposition.FILE_OPEN,
                null
            ).use { file -> file.inputStream.use { it.readBytes() } }
        }

    private fun isPhoto(name: String): Boolean =
        name.substringAfterLast('.', "").lowercase() in EXTENSIONS
}
