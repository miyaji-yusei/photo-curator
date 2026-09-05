package app.photocurator.desktop

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
    }

    @JvmStatic
    fun isConnected(): Boolean = share?.isConnected == true

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
        val disk = share ?: return null
        val relative = relativeOf(url) ?: return null
        return try {
            val info = disk.getFileInformation(relative)
            JSONObject()
                .put("size", info.standardInformation.endOfFile)
                .put("modifiedAt", info.basicInformation.lastWriteTime.toEpochMillis())
                .toString()
        } catch (error: Exception) {
            null
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
        val disk = share ?: return null
        val relative = relativeOf(url) ?: return null
        var file: File? = null
        return try {
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
            if (filled == buffer.size) buffer else buffer.copyOf(filled)
        } catch (error: Exception) {
            // 消された・権限が無い・切断された。1 枚で全体を止めない。
            null
        } finally {
            runCatching { file?.close() }
        }
    }
}
