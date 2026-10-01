package app.photocurator.next

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import uniffi.photo_curator_core.ProgressOrder

/**
 * サイドカーとこの端末の選別状況が、両方とも進んでいて違うときの確認（設計書 §4.6）。
 *
 * **5 択。既定（強調）のボタンは付けない**（どれを選ぶかは人が決める）。進んでいる側に印を
 * 付けるだけ。文言は PC・Web（SidecarConflictDialog.vue）と同じにそろえる（[SidecarSync] の定数）。
 *
 * 狭い幅（620dp 未満。Fold のカバー画面など）では 1 列に積み、広い幅では
 * 「どちらか一方（A・B・C）」と「混ぜる（D・E）」を左右に並べる。中身は縦に送れる。
 */
@Composable
fun SidecarClashDialog(
    clash: SidecarClash,
    deviceName: String,
    onChoose: (ClashChoice) -> Unit,
    onDismiss: () -> Unit
) {
    val narrow = rememberNarrow()
    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Surface(
            color = Surface,
            shape = RoundedCornerShape(16.dp),
            modifier = Modifier
                .padding(16.dp)
                .widthIn(max = if (narrow) 560.dp else 760.dp)
                .fillMaxWidth()
        ) {
            Column(
                Modifier
                    .verticalScroll(rememberScrollState())
                    .padding(horizontal = 20.dp, vertical = 18.dp)
            ) {
                Text(SidecarSync.CLASH_TITLE, fontSize = 17.sp, fontWeight = FontWeight.SemiBold)
                Spacer(Modifier.height(6.dp))
                Text(SidecarSync.reasonLine(clash), fontSize = 13.sp, color = Faint)

                Spacer(Modifier.height(14.dp))
                Side(
                    label = SidecarSync.mineLabel(deviceName),
                    line = SidecarSync.describe(clash.mine),
                    ahead = clash.order == ProgressOrder.AHEAD,
                    tint = Lime
                )
                Spacer(Modifier.height(10.dp))
                Side(
                    label = SidecarSync.theirsLabel(clash.theirs),
                    line = SidecarSync.describe(clash.theirsProgress),
                    ahead = clash.order == ProgressOrder.BEHIND,
                    tint = Sky
                )

                Spacer(Modifier.height(16.dp))
                val merge = SidecarSync.canMerge(clash.preview)
                val single: @Composable ColumnScope.() -> Unit = {
                    Choice(SidecarSync.CHOICE_TAKE_THEIRS) { onChoose(ClashChoice.TakeTheirs) }
                    Choice(SidecarSync.CHOICE_KEEP_MINE) { onChoose(ClashChoice.KeepMine) }
                    Choice(SidecarSync.CHOICE_WRITE_MINE) { onChoose(ClashChoice.WriteMine) }
                }
                val mixed: @Composable ColumnScope.() -> Unit = {
                    Choice(SidecarSync.CHOICE_INTERSECTION, SidecarSync.intersectionLine(clash.preview)) {
                        onChoose(ClashChoice.Intersection)
                    }
                    Choice(SidecarSync.CHOICE_UNION, SidecarSync.unionLine(clash.preview)) {
                        onChoose(ClashChoice.Union)
                    }
                    SidecarSync.midRoundLine(clash.preview)?.let {
                        Text(it, fontSize = 11.sp, color = Warn, modifier = Modifier.padding(top = 2.dp))
                    }
                }
                if (narrow || !merge) {
                    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        single()
                        // **片方の★1 以上が 0 枚なら混ぜる 2 つは出さない**（A か C と同じになる）。
                        if (merge) mixed()
                    }
                } else {
                    Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(8.dp)) { single() }
                        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(8.dp)) { mixed() }
                    }
                }

                Spacer(Modifier.height(14.dp))
                Text(SidecarSync.CLASH_FOOTNOTE, fontSize = 11.sp, color = Faint)
            }
        }
    }
}

/** どちらか一方の 1 行（名札と要約）。進んでいる側に印を付ける。 */
@Composable
private fun Side(label: String, line: String, ahead: Boolean, tint: Color) {
    Column {
        Text(label, fontSize = 12.sp, color = tint)
        // 印は名札の次の行に（端末名が長いと横に並べたときに途中で折れるため）。
        if (ahead) {
            Text(SidecarSync.AHEAD_MARK, fontSize = 12.sp, color = Lime, fontWeight = FontWeight.SemiBold)
        }
        Text(line, fontSize = 13.sp)
    }
}

/** 選択肢 1 つ。**どれも同じ見た目**（既定のボタンを作らない）。 */
@Composable
private fun Choice(text: String, hint: String? = null, onClick: () -> Unit) {
    Column(Modifier.fillMaxWidth()) {
        OutlinedButton(
            onClick = onClick,
            shape = RoundedCornerShape(12.dp),
            modifier = Modifier.fillMaxWidth()
        ) {
            Text(text, fontSize = 13.sp, textAlign = TextAlign.Center)
        }
        hint?.let { Text(it, fontSize = 11.sp, color = Faint, modifier = Modifier.padding(start = 4.dp, top = 2.dp)) }
    }
}
