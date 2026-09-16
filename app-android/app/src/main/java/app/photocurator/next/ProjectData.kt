package app.photocurator.next

import android.content.Context

/**
 * プロジェクトごとの持ち物の片付け先。**消す・やり直すはここにだけ書く。**
 *
 * ホームと詳細画面の両方に削除の入口があり、別々に片付けを書いていたら
 * 片方でだけ消し忘れが起きていた（設計 09 章 §5 #1）。同じ処理を 1 か所にまとめる。
 */
object ProjectData {

    /** 同じ出所を使う他のプロジェクトの数。**絵を片付けてよいかの判定に使う。** */
    suspend fun othersUsing(context: Context, project: Project): Int =
        Projects.all(context).count { it.id != project.id && it.source.key == project.source.key }

    /**
     * 消す。**選別の結果も一緒に消える。写真そのものには触らない。**
     * 指紋・顔ぶれ・つまずきは出所についての事実なので残す（別のプロジェクトでも使える）。
     */
    suspend fun remove(context: Context, project: Project) {
        Projects.remove(context, project.id)
        Store.clear(context, project.id)
        Overrides.clear(context, project.id)
        Learning.forget(context, project.id)
        Timing.clear(context, project.id)
        SyncState.forget(context, project.id)
        Prefs.forgetProject(context, project.id)
        // **最後の 1 つだったときだけ絵を片付ける。**
        val cache = project.source.cacheId
        if (cache != null && othersUsing(context, project) == 0) {
            Renders.clear(context, cache)
            // Amazon のサムネイルもこのアプリが取ってきたもの。**一緒に片付ける。**
            if (project.source.kind == "amazon") ThumbCache.clear(context, cache)
        }
    }

    /** やり直す。**星・履歴・手直し・基準・時間を消す。** 顔ぶれ・指紋・絵・サイドカーは残す。 */
    suspend fun restart(context: Context, projectId: String) {
        Store.clear(context, projectId)
        // **基準と手直しも消す。** ここを残すと、やり直しても
        // 同じまとめ方になり、聞き直す道も無くなる。
        Learning.forget(context, projectId)
        Overrides.clear(context, projectId)
        Timing.clear(context, projectId)
        // **やり直したことも判断。** 次にサイドカーへ渡す。
        SyncState.touch(context, projectId)
    }
}
