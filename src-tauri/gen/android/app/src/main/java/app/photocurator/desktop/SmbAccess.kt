package app.photocurator.desktop

import android.util.Log
import com.hierynomus.msdtyp.AccessMask
import com.hierynomus.mssmb2.SMB2CreateDisposition
import com.hierynomus.mssmb2.SMB2ShareAccess
import com.hierynomus.smbj.SMBClient
import com.hierynomus.smbj.auth.AuthenticationContext
import com.hierynomus.smbj.connection.Connection
import com.hierynomus.smbj.session.Session
import com.hierynomus.smbj.share.DiskShare
import com.hierynomus.smbj.share.File
import org.json.JSONArray
import org.json.JSONObject
import java.util.EnumSet

/**
 * NAS（SMB）への口。**Rust から JNI で静的メソッドとして呼ぶ。**
 *
 * `PhotoAccess` と同じ考え方で、境界は「一覧を返す」「バイト列を返す」だけ。
 *
 * ## なぜ smbj か
 *
 * 成熟した純 Rust の SMB クライアントが無く、`pavao` は libsmbclient への FFI で
 * Android 向けのクロスコンパイルが難しい。JVM には smbj（SMB2/3・Apache 2.0・
 * 純 Java）があり、Android で動作実績がある。
 *
 * ## 認証情報を保存しない
 *
 * パスワードは**メモリにしか置かない**。アプリを終了すると消え、次に開くときに
 * もう一度入れてもらう。NAS に繋がっているときだけ使えればよい、という方針
 * （オフライン運用はしない）なので、保存する理由がない。
 *
 * ## パスの形
 *
 * `photos.path` には `smb://<host>/<share>/<相対パス>` が入る。
 * セッションは host と share で 1 つだけ持ち、URL から引き当てる。
 */
object SmbAccess {
    private const val SCHEME = "smb://"
    /** 一度に読む最大。SMB2 の 1 リクエストの上限に収める。 */
    private const val CHUNK = 1 shl 20

    private val client = SMBClient()

    private var connection: Connection? = null
    private var session: Session? = null
    private var share: DiskShare? = null
    private var host: String = ""
    private var shareName: String = ""
    // **保存はしない。** 繋ぎ直すためにメモリにだけ持つ。プロセスが終われば消える。
    private var user: String = ""
    private var password: String = ""

    private val extensions = setOf("jpg", "jpeg", "png", "webp")

    /** 繋がっていれば空文字、駄目なら理由を返す。**例外は投げない。** */
    @JvmStatic
    @Synchronized
    fun connect(host: String, shareName: String, user: String, password: String): String {
        return try {
            disconnect()
            val connection = client.connect(host)
            val context = if (user.isEmpty()) {
                AuthenticationContext.anonymous()
            } else {
                AuthenticationContext(user, password.toCharArray(), null)
            }
            val session = connection.authenticate(context)
            val share = session.connectShare(shareName) as? DiskShare
                ?: return "共有 \"$shareName\" はフォルダとして開けません。"
            this.connection = connection
            this.session = session
            this.share = share
            this.host = host
            this.shareName = shareName
            this.user = user
            this.password = password
            ""
        } catch (error: Exception) {
            disconnect()
            error.message ?: "NAS に接続できませんでした。"
        }
    }

    @JvmStatic
    @Synchronized
    fun disconnect() {
        runCatching { share?.close() }
        runCatching { session?.close() }
        runCatching { connection?.close() }
        share = null
        session = null
        connection = null
        host = ""
        shareName = ""
        user = ""
        password = ""
    }

    @JvmStatic
    fun isConnected(): Boolean = share?.isConnected == true

    private const val TAG = "SmbAccess"

    /**
     * 同じ相手に繋ぎ直す。資格情報はメモリにあるので作り直せる。
     * 繋げたら true。
     */
    private fun revive(): Boolean {
        val host = this.host
        val shareName = this.shareName
        val user = this.user
        val password = this.password
        if (host.isEmpty() || shareName.isEmpty()) return false
        val failure = connect(host, shareName, user, password)
        if (failure.isNotEmpty()) {
            Log.w(TAG, "繋ぎ直せなかった: " + failure)
            return false
        }
        return true
    }

    /**
     * 一度だけ繋ぎ直して、同じことをやり直す。
     *
     * **SMB のセッションは黙って切れる。** NAS の省電力、Wi-Fi の切り替え、
     * アイドルの打ち切り。切れたまま読み続けると以降が全部失敗し、
     * 実機では 187 枚中 172 枚がそうなった。
     *
     * **失敗を握り潰さず必ず記録する。** 以前ここが `catch { null }` だけで、
     * 理由が一切分からず原因に辿り着けなかった。
     */
    private fun <T> retrying(what: String, body: () -> T): T? {
        try {
            return body()
        } catch (error: Exception) {
            Log.w(TAG, what + " に失敗した。繋ぎ直して試す。", error)
        }
        if (!revive()) return null
        return try {
            body()
        } catch (error: Exception) {
            Log.w(TAG, what + " は繋ぎ直しても失敗した。", error)
            null
        }
    }

    /** `smb://host/share/a/b.jpg` を共有内の相対パス `a\b.jpg` に直す。 */
    private fun relativeOf(url: String): String? {
        if (!url.startsWith(SCHEME)) return null
        val rest = url.removePrefix(SCHEME)
        val prefix = "$host/$shareName"
        if (!rest.startsWith(prefix)) return null
        return rest.removePrefix(prefix).trimStart('/').replace('/', '\\')
    }

    private fun urlOf(relative: String): String {
        val normalized = relative.replace('\\', '/').trimStart('/')
        return "$SCHEME$host/$shareName/$normalized"
    }

    /**
     * 指定フォルダの直下にあるフォルダ。**選ぶ画面のために使う。**
     * `path` が空なら共有の直下。
     */
    @JvmStatic
    @Synchronized
    fun listFolders(path: String): String {
        val disk = share ?: return "[]"
        val here = path.replace('/', '\\').trim('\\')
        val array = JSONArray()
        try {
            for (entry in disk.list(here)) {
                val name = entry.fileName
                if (name == "." || name == "..") continue
                // 属性から判定する。名前では分からない。
                val isDirectory = (entry.fileAttributes and 0x10L) != 0L
                if (!isDirectory) continue
                val child = if (here.isEmpty()) name else "$here\\$name"
                array.put(JSONObject().put("name", name).put("path", urlOf(child)))
            }
        } catch (error: Exception) {
            Log.w(TAG, "listFolders(" + here + ") に失敗した。", error)
            return "[]"
        }
        return array.toString()
    }

    /**
     * フォルダ以下の写真を**再帰的に**集める。デスクトップの WalkDir と同じ範囲。
     * 撮影時刻は EXIF から読むので、ここでは mtime と size だけ返す。
     */
    @JvmStatic
    @Synchronized
    fun listPhotos(path: String): String {
        val disk = share ?: return "[]"
        val root = relativeOf(path) ?: path.replace('/', '\\').trim('\\')
        val array = JSONArray()
        val pending = ArrayDeque<String>()
        pending.add(root)
        while (pending.isNotEmpty()) {
            val here = pending.removeFirst()
            val entries = try {
                disk.list(here)
            } catch (error: Exception) {
                // 1 つのフォルダが読めなくても、他は集める。
                Log.w(TAG, "listPhotos(" + here + ") の一覧に失敗した。", error)
                continue
            }
            for (entry in entries) {
                val name = entry.fileName
                if (name == "." || name == "..") continue
                val child = if (here.isEmpty()) name else "$here\\$name"
                val isDirectory = (entry.fileAttributes and 0x10L) != 0L
                if (isDirectory) {
                    pending.add(child)
                    continue
                }
                val extension = name.substringAfterLast('.', "").lowercase()
                if (extension !in extensions) continue
                array.put(
                    JSONObject()
                        .put("uri", urlOf(child))
                        .put("name", name)
                        .put("relativePath", child.replace('\\', '/'))
                        .put("size", entry.endOfFile)
                        // SMB の時刻は Windows FILETIME。smbj がミリ秒に直してくれる。
                        .put("modifiedAt", entry.lastWriteTime.toEpochMillis())
                )
            }
        }
        return array.toString()
    }

    @JvmStatic
    @Synchronized
    fun stat(url: String): String? {
        val relative = relativeOf(url) ?: return null
        return retrying("stat(" + relative + ")") {
            val disk = share ?: throw IllegalStateException("NAS に繋がっていません。")
            val info = disk.getFileInformation(relative)
            JSONObject()
                .put("size", info.standardInformation.endOfFile)
                .put("modifiedAt", info.basicInformation.lastWriteTime.toEpochMillis())
                .toString()
        }
    }

    /**
     * 写真のバイト列。`length` が 0 以下なら最後まで。
     *
     * **部分読みができることが要点。** EXIF 埋め込みサムネイル経路は先頭 26KB
     * 程度で済む（実測）。ここを常に全体にすると 1 枚 6.7MB を毎回 Wi-Fi で
     * 運ぶことになり、2,000 枚で 13.4GB になる。
     */
    @JvmStatic
    @Synchronized
    fun readBytes(url: String, offset: Long, length: Int): ByteArray? {
        val relative = relativeOf(url) ?: return null
        return retrying("readBytes(" + relative + ")") { readOnce(relative, offset, length) }
    }

    /** 1 回ぶんの読み出し。失敗は例外のまま上へ返す（`retrying` が拾う）。 */
    private fun readOnce(relative: String, offset: Long, length: Int): ByteArray {
        val disk = share ?: throw IllegalStateException("NAS に繋がっていません。")
        var file: File? = null
        try {
            file = disk.openFile(
                relative,
                EnumSet.of(AccessMask.GENERIC_READ),
                null,
                SMB2ShareAccess.ALL,
                SMB2CreateDisposition.FILE_OPEN,
                null
            )
            val total = file.fileInformation.standardInformation.endOfFile
            val want = if (length <= 0) (total - offset) else minOf(length.toLong(), total - offset)
            if (want <= 0) return ByteArray(0)
            val buffer = ByteArray(want.toInt())
            var filled = 0
            while (filled < buffer.size) {
                val slice = minOf(CHUNK, buffer.size - filled)
                val read = file.read(buffer, offset + filled, filled, slice)
                if (read <= 0) break
                filled += read
            }
            return if (filled == buffer.size) buffer else buffer.copyOf(filled)
        } finally {
            runCatching { file?.close() }
        }
    }
}
