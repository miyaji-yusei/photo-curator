package app.photocurator.next

import android.app.Activity
import android.content.ContentValues
import android.content.Context
import android.content.IntentSender
import android.os.Build
import android.provider.MediaStore
import android.util.Log
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * 選別の結果を写真そのものへ返す（「取り出す」）。
 *
 * **ここだけが原本に手を入れる。** 選別中はどこも書かない。
 * 書き込みは必ず端末の同意画面を通す。`createFavoriteRequest` は
 * 「この N 枚をお気に入りにしますか」を OS が出し、利用者が押して初めて
 * 反映される。アプリが黙って書き換える経路は作らない。
 */
object Take {
    private const val TAG = "Take"

    /**
     * お気に入りを付ける要求。**押す前の確認は OS がやる。**
     *
     * 端末が対応していない（API 30 未満）ときは null。
     * 呼んだ側は理由を出すこと。**黙って何も起きないのが一番困る。**
     */
    fun favouriteRequest(context: Context, photos: List<Photo>): IntentSender? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) {
            Log.w(TAG, "この端末ではお気に入りを付けられない（API ${Build.VERSION.SDK_INT}）")
            return null
        }
        if (photos.isEmpty()) return null
        return try {
            MediaStore.createFavoriteRequest(
                context.contentResolver,
                photos.map { it.uri },
                true
            ).intentSender
        } catch (error: Exception) {
            Log.w(TAG, "お気に入りの要求を作れなかった", error)
            null
        }
    }

    /** 同意画面の結果を、そのまま言葉にする。**「失敗」と「やめた」を分ける。** */
    fun describe(resultCode: Int, count: Int): String = when (resultCode) {
        Activity.RESULT_OK -> "$count 枚にお気に入りを付けました"
        Activity.RESULT_CANCELED -> "お気に入りは付けていません（取り消されました）"
        else -> "お気に入りを付けられませんでした"
    }
    /**
     * 別のフォルダへ移す許可を求める。**書き込みは許可が下りてから。**
     *
     * `createWriteRequest` は「この N 枚を変更してよいか」を OS に尋ねる。
     * ただし**画面が出ないことがある**（すでに許可を持っていると黙って通る）。
     * だから呼ぶ側は、この前に自前の確認を必ず挟むこと。
     */
    fun moveRequest(context: Context, photos: List<Photo>): IntentSender? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) {
            Log.w(TAG, "この端末では移せない（API ${Build.VERSION.SDK_INT}）")
            return null
        }
        if (photos.isEmpty()) return null
        return try {
            MediaStore.createWriteRequest(context.contentResolver, photos.map { it.uri })
                .intentSender
        } catch (error: Exception) {
            Log.w(TAG, "移す許可の要求を作れなかった", error)
            null
        }
    }

    /** 移した結果。**成功だけでなく、落ちた分と理由を持って帰る。** */
    data class Moved(val moved: Int, val failed: Int, val firstReason: String?) {
        fun describe(destination: String): String = when {
            moved > 0 && failed == 0 -> "$moved 枚を「$destination」へ移しました"
            moved > 0 -> "$moved 枚を移しました。$failed 枚は移せませんでした（${firstReason ?: "理由不明"}）"
            else -> "移せませんでした（${firstReason ?: "理由不明"}）"
        }
    }

    /**
     * 実際に移す。**RELATIVE_PATH を書き換えると、原本の置き場所が変わる。**
     *
     * 1 枚ずつ書く。まとめて 1 回で書ければ速いが、途中で 1 枚失敗したときに
     * **どこまで移ったのか分からなくなる。** 移動は戻しにくいので、
     * 数と理由をはっきりさせる方を採る。
     */
    suspend fun move(
        context: Context,
        photos: List<Photo>,
        destinationDir: String
    ): Moved = withContext(Dispatchers.IO) {
        // MediaStore は末尾の "/" を要求する。無いと黙って別の場所になる。
        val dir = if (destinationDir.endsWith("/")) destinationDir else "$destinationDir/"
        var moved = 0
        var failed = 0
        var firstReason: String? = null
        for (photo in photos) {
            try {
                val values = ContentValues().apply {
                    put(MediaStore.MediaColumns.RELATIVE_PATH, dir)
                }
                val rows = context.contentResolver.update(photo.uri, values, null, null)
                if (rows > 0) moved += 1 else {
                    failed += 1
                    if (firstReason == null) firstReason = "${photo.name} は変更されませんでした"
                }
            } catch (error: Exception) {
                failed += 1
                // **1 枚で全体を止めない。理由は必ず残す。**
                Log.w(TAG, "移せなかった: ${photo.name}", error)
                if (firstReason == null) {
                    firstReason = error.message?.take(80) ?: error.javaClass.simpleName
                }
            }
        }
        Moved(moved, failed, firstReason)
    }
}
