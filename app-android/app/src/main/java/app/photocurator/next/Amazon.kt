package app.photocurator.next

import android.util.Log
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import okhttp3.Request
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit

/**
 * Amazon Photos の**共有リンク**を読む。**ログインはしない。**
 *
 * 公式の API は無い。共有ページ自身が読んでいる JSON（`/drive/v1/...`）を、
 * リンクに入っている shareId だけで読む（設計 08 章）。アカウントの認証情報は
 * 一切使わないので、うまくいかなくても「このリンクが読めない」で済む。
 *
 * **規約の約束**（設計 08 章 13）: 本人が渡したリンクだけ／本人の操作のときだけ／
 * 並列 4 まで／同じ絵は二度取らない／Amazon の画面を取り込まない。
 */
object Amazon {
    private const val TAG = "Amazon"

    /** 一覧は 200 件ずつ。**これより大きいと 400 が返る**（実測）。 */
    private const val PAGE = 200

    /** 並べて取る数。**実測では 8 で問題ないが、通常の利用に収めるため 4。** */
    const val PARALLEL = 4

    /** 一覧用の縮小。長辺 160 で約 7KB（実測）。 */
    const val THUMB = 160

    /** 潜る深さ。共有 → アルバム → 写真 で 2。 */
    private const val DEPTH = 2

    /** リンクが消えたときの言い方。**作成時も作成後も同じ文にする。** */
    const val GONE = "このリンクは削除されたか、無効です"

    private val LINK =
        Regex("https://(www\\.amazon\\.[a-z.]+)/(?:photos|clouddrive)/share/([A-Za-z0-9_-]+)")

    /** リンクから読み取った指し先。 */
    data class Link(val host: String, val shareId: String) {
        /** 出所の鍵。 */
        val key: String get() = "$host|$shareId"

        /** 端末に置く絵の名前の頭。**NAS の id と同じ欄に入れる。** */
        val cacheId: String get() = "amz-$shareId"

        /** 人に見せる形。貼り付け欄に出す。 */
        val url: String get() = "https://$host/photos/share/$shareId"
    }

    /** 文の中から共有リンクを拾う。共有で受け取った本文にも使う。 */
    fun parse(text: String): Link? =
        LINK.find(text)?.let { Link(it.groupValues[1], it.groupValues[2]) }

    fun linkOf(key: String): Link = Link(key.substringBefore("|"), key.substringAfter("|"))

    data class Share(val name: String, val rootId: String)

    /** 一覧の 1 件。**写真だけ**（動画・フォルダは入れない）。 */
    data class Item(
        val nodeId: String,
        val name: String,
        val takenAt: Long,
        val size: Long,
        val tempLink: String
    )

    private val client: OkHttpClient by lazy {
        OkHttpClient.Builder()
            .connectTimeout(10, TimeUnit.SECONDS)
            .readTimeout(30, TimeUnit.SECONDS)
            // tempLink は 302 で本体へ飛ぶ。
            .followRedirects(true)
            .build()
    }

    /** HTTP の番号を持った失敗。**404 はリンクが消えた印。** */
    private class Http(val code: Int) : java.io.IOException("HTTP $code")

    private fun get(url: String): ByteArray {
        client.newCall(Request.Builder().url(url).build()).execute().use { response ->
            if (!response.isSuccessful) throw Http(response.code)
            return response.body?.bytes() ?: throw Http(response.code)
        }
    }

    /** 混んでいるときだけ、2 秒待って 1 回やり直す。**何度も叩かない。** */
    private suspend fun getPolitely(url: String): ByteArray = try {
        get(url)
    } catch (error: Http) {
        if (error.code != 429 && error.code != 503) throw error
        delay(2_000)
        get(url)
    }

    private fun describe(error: Exception): String = when {
        error is Http && error.code == 404 -> GONE
        error is Http -> "Amazon Photos を読めませんでした（${error.code}）"
        // 網が無い・名前が引けないなどは NAS と同じ言い方にする。
        else -> Smb.describe(error)
    }

    suspend fun share(link: Link): SmbResult<Share> = withContext(Dispatchers.IO) {
        try {
            val json = org.json.JSONObject(
                String(
                    getPolitely(
                        "https://${link.host}/drive/v1/shares/${link.shareId}" +
                            "?shareId=${link.shareId}&resourceVersion=V2&ContentType=JSON"
                    )
                )
            )
            val node = json.getJSONObject("nodeInfo")
            SmbResult.Ok(Share(node.optString("name", ""), node.getString("id")))
        } catch (error: Exception) {
            Log.w(TAG, "共有を読めなかった", error)
            SmbResult.Failed(describe(error))
        }
    }

    /**
     * 共有の中の写真を全部。**撮影時刻の昇順**で返す。
     *
     * 共有の直下はアルバムとは限らない（今回は 共有 → アルバム → 写真）。
     * FILE 以外は 2 階層まで潜る。
     */
    suspend fun photos(link: Link): SmbResult<List<Item>> = withContext(Dispatchers.IO) {
        val share = when (val got = share(link)) {
            is SmbResult.Failed -> return@withContext got
            is SmbResult.Ok -> got.value
        }
        val found = ArrayList<Item>()

        suspend fun walk(nodeId: String, depth: Int) {
            var offset = 0
            while (true) {
                val json = org.json.JSONObject(
                    String(
                        getPolitely(
                            "https://${link.host}/drive/v1/nodes/$nodeId/children" +
                                "?asset=ALL&limit=$PAGE&offset=$offset&searchOnFamily=false" +
                                "&tempLink=true&shareId=${link.shareId}" +
                                "&sort=%5B%27contentProperties.contentDate+ASC%27%5D" +
                                "&resourceVersion=V2&ContentType=JSON"
                        )
                    )
                )
                val data = json.optJSONArray("data") ?: break
                for (at in 0 until data.length()) {
                    val node = data.getJSONObject(at)
                    if (node.optString("kind") != "FILE") {
                        if (depth < DEPTH) walk(node.getString("id"), depth + 1)
                        continue
                    }
                    val content = node.optJSONObject("contentProperties") ?: continue
                    // **拡張子では決めない。** 名前が .cr2 でも中身が JPEG のことがある
                    // （実測で 454 枚すべて）。動画もここで落ちる。
                    if (!content.optString("contentType").startsWith("image/")) continue
                    val temp = node.optString("tempLink")
                    if (temp.isEmpty()) continue
                    found += Item(
                        nodeId = node.getString("id"),
                        name = node.optString("name"),
                        takenAt = dateOf(content.optString("contentDate")),
                        size = content.optLong("size"),
                        tempLink = temp
                    )
                }
                offset += data.length()
                if (data.length() == 0 || offset >= json.optInt("count", 0)) break
            }
        }

        try {
            walk(share.rootId, 0)
            // **使う値そのもので並べる。** 同時刻は node id で決める（毎回同じ順）。
            SmbResult.Ok(found.sortedWith(compareBy({ it.takenAt }, { it.nodeId })))
        } catch (error: Exception) {
            Log.w(TAG, "一覧を読めなかった", error)
            SmbResult.Failed(describe(error))
        }
    }

    private fun dateOf(text: String): Long = try {
        java.time.Instant.parse(text).toEpochMilli()
    } catch (error: Exception) {
        0L
    }

    /** 取り直した tempLink。**使えなかったときだけ**ここに入る。node id → URL */
    private val fresh = ConcurrentHashMap<String, String>()

    /** 最後に一覧を取り直した時刻。**失敗が続いても叩き続けない。** */
    private val relisted = ConcurrentHashMap<String, Long>()

    /**
     * 写真を 1 枚。`box` が null なら原本、あれば長辺をその大きさにした JPEG。
     *
     * tempLink が使えなかったら、一覧を 1 回だけ取り直してやり直す（1 分に 1 回まで）。
     * リンク自体が消えていたら [GONE] を返す。
     */
    suspend fun image(ref: AmazonRef, box: Int?): SmbResult<ByteArray> =
        withContext(Dispatchers.IO) {
            val first = fresh[ref.nodeId] ?: ref.tempLink
            try {
                SmbResult.Ok(getPolitely(sized(first, box)))
            } catch (error: Exception) {
                val stale = error is Http && error.code in setOf(403, 404, 410)
                val now = System.currentTimeMillis()
                val last = relisted[ref.shareKey] ?: 0L
                if (stale && now - last > 60_000) {
                    relisted[ref.shareKey] = now
                    when (val again = photos(linkOf(ref.shareKey))) {
                        is SmbResult.Failed -> return@withContext again
                        is SmbResult.Ok -> again.value.forEach { fresh[it.nodeId] = it.tempLink }
                    }
                    val next = fresh[ref.nodeId]
                    if (next != null && next != first) {
                        return@withContext try {
                            SmbResult.Ok(getPolitely(sized(next, box)))
                        } catch (retry: Exception) {
                            SmbResult.Failed(describe(retry))
                        }
                    }
                }
                Log.w(TAG, "画像を取れなかった: ${ref.nodeId}", error)
                SmbResult.Failed(describe(error))
            }
        }

    private fun sized(url: String, box: Int?) = if (box == null) url else "$url?viewBox=$box,$box"

    /**
     * Amazon が出せる長辺の上限を測る。**1 枚だけ、選択肢の最大を頼んで、
     * 返ってきた長辺を見る**（設計 08 章 8.5）。測れなければ null。
     */
    suspend fun measureMaxEdge(ref: AmazonRef): Int? {
        val bytes = (image(ref, Prefs.EDGES.max()) as? SmbResult.Ok)?.value ?: return null
        val bounds = android.graphics.BitmapFactory.Options().apply { inJustDecodeBounds = true }
        android.graphics.BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
        val longest = maxOf(bounds.outWidth, bounds.outHeight)
        return longest.takeIf { it > 0 }
    }
}
