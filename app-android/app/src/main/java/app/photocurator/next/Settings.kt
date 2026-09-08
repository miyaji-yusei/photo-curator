package app.photocurator.next

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * 設定。**NAS と、選別の既定値。**
 *
 * ここは「次に作るプロジェクトがどう始まるか」を決める場所。
 * いま動いている選別は変えない（選別中の変更は選別画面の `…` から）。
 */
@Composable
fun SettingsScreen(onBack: () -> Unit) {
    val context = LocalContext.current
    var groupSize by remember { mutableStateOf(Prefs.groupSize(context)) }
    var groupBursts by remember { mutableStateOf(Prefs.groupBursts(context)) }
    var askBeforeStart by remember { mutableStateOf(Prefs.askBeforeStart(context)) }

    Column(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing)) {
        Row(
            Modifier.fillMaxWidth().height(64.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            IconButton(onClick = onBack) { Icon(Icons.Filled.ArrowBack, "戻る") }
            Text("設定", fontSize = 18.sp, fontWeight = FontWeight.SemiBold)
        }

        Row(
            Modifier.fillMaxSize().padding(horizontal = 16.dp),
            horizontalArrangement = Arrangement.spacedBy(16.dp)
        ) {
            Column(Modifier.weight(1f)) {
                Panel {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text("NAS", fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
                        Spacer(Modifier.weight(1f))
                        TextButton(onClick = { }, enabled = false) {
                            Icon(Icons.Filled.Add, null, Modifier.size(16.dp))
                            Spacer(Modifier.width(4.dp))
                            Text("追加", fontSize = 13.sp)
                        }
                    }
                    Spacer(Modifier.height(8.dp))
                    // **「作る予定」ではなく「まだできない」と書く。**
                    // 押せるのに何も起きないボタンを置くより、理由を出す。
                    Text(
                        "まだ繋げません。接続の確認に実機の NAS が要るため、" +
                            "繋いだ状態で一緒に作ります。",
                        fontSize = 12.sp, color = Faint
                    )
                    Spacer(Modifier.height(8.dp))
                    Text(
                        "写真は端末にコピーしません。必要な部分だけ NAS から読みます",
                        fontSize = 11.sp, color = Faint
                    )
                }
            }

            Column(Modifier.weight(1f)) {
                Panel {
                    Text("選別の既定", fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
                    Spacer(Modifier.height(12.dp))

                    Text("一度に見比べる枚数", fontSize = 13.sp)
                    Spacer(Modifier.height(6.dp))
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        for (size in listOf(2, 3, 4)) {
                            FilterChip(
                                selected = size == groupSize,
                                onClick = { groupSize = size; Prefs.setGroupSize(context, size) },
                                label = { Text("$size", fontSize = 13.sp) },
                                colors = FilterChipDefaults.filterChipColors(
                                    selectedContainerColor = Lime, selectedLabelColor = Color.Black
                                )
                            )
                        }
                    }

                    Spacer(Modifier.height(16.dp))
                    Toggle("連写を自動でまとめる", groupBursts) {
                        groupBursts = it; Prefs.setGroupBursts(context, it)
                    }
                    Toggle("開始前に毎回この設定を確認する", askBeforeStart) {
                        askBeforeStart = it; Prefs.setAskBeforeStart(context, it)
                    }

                    Spacer(Modifier.height(16.dp))
                    Row(verticalAlignment = Alignment.Top) {
                        Icon(Icons.Filled.Lock, null, Modifier.size(14.dp), tint = Faint)
                        Spacer(Modifier.width(6.dp))
                        Text(
                            "Photo Curator は写真の原本を移動・削除・書き換えしません。" +
                                "星の書き込みとフォルダ分けは、実行前に確認します。",
                            fontSize = 11.sp, color = Faint
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun Panel(content: @Composable ColumnScope.() -> Unit) {
    Column(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(14.dp))
            .background(Surface)
            .padding(16.dp),
        content = content
    )
}

@Composable
private fun Toggle(label: String, on: Boolean, onChange: (Boolean) -> Unit) {
    Row(
        Modifier.fillMaxWidth().padding(vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Text(label, fontSize = 13.sp, modifier = Modifier.weight(1f))
        Switch(
            checked = on, onCheckedChange = onChange,
            colors = SwitchDefaults.colors(
                checkedThumbColor = Color.Black, checkedTrackColor = Lime
            )
        )
    }
}
