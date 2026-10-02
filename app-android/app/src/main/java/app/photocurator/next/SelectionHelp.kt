package app.photocurator.next

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.clickable
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * 選別中の「？」ヘルプの文章（U24）。Android の操作の一覧は**この 1 つの関数**に書く。
 * 実際の操作（`Slideshow.kt`・`SlideshowGesture.kt`・`Cull.kt` の Tile・CullBar）と食い違わないように直すこと。
 */
data class HelpSection(val title: String, val items: List<String>)

data class HelpGuide(val title: String, val sections: List<HelpSection>)

fun selectionHelp(method: Method): HelpGuide = when (method) {
    Method.Slideshow -> HelpGuide(
        "スライドショーの操作",
        listOf(
            HelpSection(
                "タップ",
                listOf(
                    "写真の左のほうをタップ: 落とす",
                    "写真の右のほうをタップ: 残す",
                    "写真の上のほうをタップ: ★5 で確定",
                    "写真の中央のタップ: 何もしない",
                    "写真の中央を二度タップ: 拡大",
                    "右上の ★ ボタン: ★5 で確定、虫眼鏡のボタン: 拡大",
                    "左上の「連写 n 枚」: まとめられた写真を開く"
                )
            ),
            HelpSection(
                "フリック（スワイプ）",
                listOf(
                    "左へスワイプ: 落とす",
                    "右へスワイプ: 残す",
                    "上へスワイプ: ★5 で確定",
                    "下向きは使いません",
                    "長押しの拡大はありません"
                )
            ),
            HelpSection(
                "上のバー",
                listOf("↩: 1 つ戻す", "…: 選別中の設定（方式・連写・表示用画像の大きさ）")
            )
        )
    )
    Method.Tournament -> HelpGuide(
        "トーナメントの操作",
        listOf(
            HelpSection(
                "写真",
                listOf(
                    "写真をタップ: その写真を残して次へ（複数選択中は、選ぶ印の切り替え）",
                    "写真を長押し: 複数選択を始めて、その写真を選ぶ（アプリの設定で、長押しを拡大に変えられます）",
                    "右上の ★ ボタン: その 1 枚を ★5 で確定",
                    "右上の虫眼鏡のボタン: 拡大",
                    "左上の「連写 n 枚」: まとめられた写真を開く"
                )
            ),
            HelpSection(
                "上のバー",
                listOf(
                    "↩: 1 つ戻す",
                    "複数選択のボタン: 複数の写真を選べるようにする",
                    "「n 枚を残す」「落として次へ」: 選んだ写真を残して確定（何も選んでいなければ、全部落とす）",
                    "隣り合う写真を 2 枚以上選ぶと「ひとまとまりにする」が出ます",
                    "…: 選別中の設定（方式・枚数・連写・表示用画像の大きさ）"
                )
            )
        )
    )
}

/** 「？」を押したときのダイアログ。今の方式の一覧を出し、もう一方の方式は折りたたんでおく。 */
@Composable
fun SelectionHelpDialog(slideshow: Boolean, onDismiss: () -> Unit) {
    val current = selectionHelp(if (slideshow) Method.Slideshow else Method.Tournament)
    val other = selectionHelp(if (slideshow) Method.Tournament else Method.Slideshow)
    var otherOpen by remember { mutableStateOf(false) }
    AlertDialog(
        onDismissRequest = onDismiss,
        containerColor = Surface,
        title = { Text(current.title, fontSize = 17.sp, fontWeight = FontWeight.SemiBold) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                HelpBody(current)
                Row(
                    Modifier
                        .fillMaxWidth()
                        .clickable { otherOpen = !otherOpen }
                        .padding(top = 12.dp, bottom = 8.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text(
                        "${other.title}（もう一方の方式）",
                        fontSize = 13.sp, fontWeight = FontWeight.SemiBold, color = Lime,
                        modifier = Modifier.weight(1f)
                    )
                    Text(if (otherOpen) "たたむ" else "ひらく", fontSize = 12.sp, color = Faint)
                }
                if (otherOpen) HelpBody(other)
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text("閉じる") } }
    )
}

@Composable
private fun HelpBody(guide: HelpGuide) {
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        for (section in guide.sections) {
            Column {
                Text(section.title, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                for (item in section.items) {
                    Text("・$item", fontSize = 12.sp, color = Faint, modifier = Modifier.padding(top = 3.dp))
                }
            }
        }
    }
}
