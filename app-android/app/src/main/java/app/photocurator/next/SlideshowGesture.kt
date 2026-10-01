package app.photocurator.next

import kotlin.math.abs
import kotlin.math.hypot
import kotlin.math.max
import kotlin.math.min

/**
 * スライドショー選別の操作の判定。**見た目と入力の解釈だけ**で、残す・落とすの
 * 判断はここでしない（決まった操作は core の `advance`・`keepAndTop` へ渡す）。
 *
 * PC・Web の `utils/slideshowGesture.ts` と**同じ数値・同じ規則**。長さは dp
 * （Web の px と同じ大きさ）で受ける。画面の px は呼ぶ側が密度で割ってから渡す。
 *
 * 操作: 左＝落とす／右＝残す／上＝★5 で確定。下は使わない。
 * タップ: 上の帯＝★5、ほぼ中心＝何もしない（二度タップで拡大）、
 * それ以外は左半分＝落とす・右半分＝残す。
 */
enum class SlideDecision { Drop, Keep, Top }

object SlideshowGesture {
    /** これ未満の動きはドラッグでなくタップとして扱う。 */
    const val TAP_SLOP = 8f

    /** 決定になるドラッグ量。幅の 18%、ただし 60〜160 の範囲。 */
    fun decideThreshold(width: Float): Float {
        if (!width.isFinite()) return 60f
        return min(160f, max(60f, width * 0.18f))
    }

    fun isTap(dx: Float, dy: Float): Boolean = hypot(dx, dy) < TAP_SLOP

    /** ドラッグを離したときの判定。量が足りない・下向きなら null（元へ戻す）。 */
    fun judgeDrag(dx: Float, dy: Float, width: Float): SlideDecision? {
        val threshold = decideThreshold(width)
        val ax = abs(dx)
        val ay = abs(dy)
        if (dy < 0 && ay > ax && ay >= threshold) return SlideDecision.Top
        if (ax >= threshold && ax >= ay) return if (dx < 0) SlideDecision.Drop else SlideDecision.Keep
        return null
    }

    /**
     * ドラッグ中の見た目。
     * @property direction いま出す色の膜・アイコン・文字。null なら何も出さない。
     * @property strength 0〜1。決定になるドラッグ量で 1。
     * @property rotation 写真の回転（度）。
     */
    data class Feedback(val direction: SlideDecision?, val strength: Float, val rotation: Float)

    /** 決定の向きと同じ規則（縦が優位で上向きなら「上」、横が優位なら左右）。 */
    fun dragFeedback(dx: Float, dy: Float, width: Float): Feedback {
        val threshold = decideThreshold(width)
        val ax = abs(dx)
        val ay = abs(dy)
        val span = max(1f, width)
        val rotation = max(-15f, min(15f, (dx / span) * 30f))
        if (dy < 0 && ay > ax) return Feedback(SlideDecision.Top, min(1f, ay / threshold), rotation)
        if (ax > 0 && ax >= ay) {
            return Feedback(
                if (dx < 0) SlideDecision.Drop else SlideDecision.Keep,
                min(1f, ax / threshold), rotation
            )
        }
        return Feedback(null, 0f, rotation)
    }

    /** 枠の上から、この割合までの帯のタップは「★5 で確定」。 */
    const val TOP_BAND_RATIO = 0.25f

    /** 枠の中心から、横は幅の ±12%・縦は高さの ±12%（境目を含む）の長方形が「ほぼ中心」。 */
    const val CENTER_RATIO = 0.12f

    /**
     * タップの判定。枠の上の帯（高さの上から 25% 未満）は「★5 で確定」（帯が優先）、
     * ほぼ中心は **null（何もしない。二度タップなら拡大）**、
     * それ以外は左半分が「落とす」・右半分が「残す」。
     */
    fun tapDecision(
        x: Float, y: Float, left: Float, top: Float, width: Float, height: Float
    ): SlideDecision? {
        if (y < top + height * TOP_BAND_RATIO) return SlideDecision.Top
        val cx = left + width / 2
        val cy = top + height / 2
        if (abs(x - cx) <= width * CENTER_RATIO && abs(y - cy) <= height * CENTER_RATIO) return null
        return if (x < cx) SlideDecision.Drop else SlideDecision.Keep
    }

    /** 二度タップとみなす間隔（ms）と距離（dp）。どちらも境目を含む。 */
    const val DOUBLE_TAP_MS = 300L
    const val DOUBLE_TAP_DISTANCE = 24f

    /** 中心タップの記録。時刻は ms、位置は dp。 */
    data class TapRecord(val timeMs: Long, val x: Float, val y: Float)

    /** 直前の中心タップ（なければ null）と今回の中心タップが、300ms 以内・24dp 以内なら二度タップ。 */
    fun isDoubleTap(prev: TapRecord?, now: TapRecord): Boolean {
        if (prev == null) return false
        val dt = now.timeMs - prev.timeMs
        if (dt < 0 || dt > DOUBLE_TAP_MS) return false
        return hypot(now.x - prev.x, now.y - prev.y) <= DOUBLE_TAP_DISTANCE
    }

    /** 決定したとき、写真が飛んでいく先。 */
    data class Fly(val x: Float, val y: Float, val rotation: Float)

    fun flyTarget(decision: SlideDecision, width: Float, height: Float): Fly {
        if (decision == SlideDecision.Top) return Fly(0f, -height * 1.2f, 0f)
        val sign = if (decision == SlideDecision.Drop) -1f else 1f
        return Fly(sign * width * 1.2f, 0f, sign * 20f)
    }

    /** 縦横比を保って枠に収まる最大の大きさ（枠より小さい写真は拡大する）。 */
    fun fitContain(
        naturalWidth: Float, naturalHeight: Float, frameWidth: Float, frameHeight: Float
    ): Pair<Float, Float> {
        if (!(naturalWidth > 0) || !(naturalHeight > 0) || !(frameWidth > 0) || !(frameHeight > 0)) {
            return max(0f, frameWidth) to max(0f, frameHeight)
        }
        val scale = min(frameWidth / naturalWidth, frameHeight / naturalHeight)
        return naturalWidth * scale to naturalHeight * scale
    }
}
