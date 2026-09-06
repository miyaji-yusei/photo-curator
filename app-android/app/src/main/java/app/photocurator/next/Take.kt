package app.photocurator.next

import android.app.Activity
import android.content.Context
import android.content.IntentSender
import android.os.Build
import android.provider.MediaStore
import android.util.Log

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
}
