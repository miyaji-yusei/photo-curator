package app.photocurator.next

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.sp

/**
 * 選別の方式（U24）。PC・Web の MethodView と同じ「トーナメント／スライドショー」の 2 択。
 *
 * 保存される値は枚数のまま（`group_size`）。**1 がスライドショー、2〜10 がトーナメント。**
 * ここは「方式と枚数」の組を扱う純粋な部分で、画面（Start・Options・Settings）が共通で使う。
 * Compose に依存しないので JUnit で確かめられる（`SelectionMethodTest`）。
 */
enum class Method { Tournament, Slideshow }

/** 方式の説明（スライドショー）。開始前・選別中の設定・アプリ設定で同じ文を出す。 */
const val SLIDESHOW_NOTE =
    "1 枚ずつ出して、残す（右）か落とす（左）かを決めます。" +
        "上へのスワイプや、写真の上のほうのタップで ★5 にして確定します。" +
        "ほぼ中心のタップは何もせず、二度タップで拡大します"

/**
 * 画面で選んでいる途中の「方式」と「トーナメントの枚数」。
 * 方式を切り替えても、トーナメントの枚数は覚えておく（戻したときに元の枚数になる）。
 */
data class PendingMethod(val method: Method, val tournamentSize: Int) {
    /** 確定したときに保存・適用する枚数（`group_size`）。 */
    val size: Int
        get() = if (method == Method.Slideshow) Prefs.SLIDESHOW_SIZE else tournamentSize

    fun withMethod(next: Method) = copy(method = next)

    /** トーナメントの枚数を選ぶ。2〜10 に収め、方式はトーナメントになる。 */
    fun withSize(next: Int) = PendingMethod(Method.Tournament, next.coerceIn(2, 10))

    /** 確定ボタンの文言。PC・Web の GroupSizeDialog と同じ。 */
    val confirmLabel: String
        get() = if (method == Method.Slideshow) "この方式にする" else "この枚数にする"

    /** いま動いている枚数（`group_size`）と違うか。 */
    fun differsFrom(groupSize: Int): Boolean = size != groupSize

    companion object {
        /** 保存済みの枚数（1 ならスライドショー）と、直前のトーナメントの枚数から。 */
        fun from(groupSize: Int, lastTournamentSize: Int): PendingMethod {
            val tournament = lastTournamentSize.coerceIn(2, 10)
            return if (groupSize == Prefs.SLIDESHOW_SIZE) PendingMethod(Method.Slideshow, tournament)
            else PendingMethod(Method.Tournament, groupSize.coerceIn(2, 10))
        }
    }
}

/** 方式の 2 択（トーナメントが左）。開始前・選別中の設定・アプリ設定で同じ部品。 */
@Composable
fun MethodSegment(method: Method, onChange: (Method) -> Unit, modifier: Modifier = Modifier) {
    val colors = SegmentedButtonDefaults.colors(
        activeContainerColor = Lime, activeContentColor = Color.Black
    )
    SingleChoiceSegmentedButtonRow(modifier.fillMaxWidth()) {
        SegmentedButton(
            selected = method == Method.Tournament,
            onClick = { onChange(Method.Tournament) },
            shape = SegmentedButtonDefaults.itemShape(0, 2),
            colors = colors
        ) { Text("トーナメント", fontSize = 13.sp) }
        SegmentedButton(
            selected = method == Method.Slideshow,
            onClick = { onChange(Method.Slideshow) },
            shape = SegmentedButtonDefaults.itemShape(1, 2),
            colors = colors
        ) { Text("スライドショー", fontSize = 13.sp) }
    }
}
