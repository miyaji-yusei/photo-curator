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
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.filled.ChevronRight
import kotlinx.coroutines.launch
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
    var displayEdge by remember { mutableStateOf(Prefs.displayEdge(context)) }
    var detailing by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    var nasList by remember { mutableStateOf<List<Nas>>(emptyList()) }
    // 編集中のつなぎ先。null で新規、Nas で既存。
    var editing by remember { mutableStateOf<Nas?>(null) }
    var adding by remember { mutableStateOf(false) }
    var reloads by remember { mutableStateOf(0) }

    LaunchedEffect(reloads) { nasList = NasStore.all(context) }

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
                        TextButton(onClick = { adding = true }) {
                            Icon(Icons.Filled.Add, null, Modifier.size(16.dp))
                            Spacer(Modifier.width(4.dp))
                            Text("追加", fontSize = 13.sp)
                        }
                    }
                    Spacer(Modifier.height(8.dp))
                    if (nasList.isEmpty()) {
                        Text(
                            "まだ登録がありません。「追加」から、ホスト名・共有名・" +
                                "ユーザー名を入れてください。",
                            fontSize = 12.sp, color = Faint
                        )
                    }
                    LazyColumn(Modifier.heightIn(max = 260.dp)) {
                        items(nasList, key = { it.id }) { nas ->
                            Row(
                                Modifier
                                    .fillMaxWidth()
                                    .clip(RoundedCornerShape(10.dp))
                                    .clickable { editing = nas }
                                    .padding(vertical = 10.dp, horizontal = 4.dp),
                                verticalAlignment = Alignment.CenterVertically
                            ) {
                                Column(Modifier.weight(1f)) {
                                    Text(nas.label, fontSize = 14.sp)
                                    // **ホストは人には見せない…のではなく、**
                                    // ここは設定なので出す。普段の画面には出さない。
                                    Text(
                                        "${nas.host} · ${nas.share} · ${nas.user}" +
                                            if (nas.remember) " · パスワード保存あり" else "",
                                        fontSize = 12.sp, color = Faint
                                    )
                                }
                                Icon(Icons.Filled.ChevronRight, null, Modifier.size(18.dp), tint = Faint)
                            }
                        }
                    }
                    Spacer(Modifier.height(8.dp))
                    Text(
                        "写真は端末にコピーしません。必要な部分だけ NAS から読みます",
                        fontSize = 11.sp, color = Faint
                    )
                }
            }

            Column(Modifier.weight(1f)) {
                Panel {
                    Text("表示用画像の既定", fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
                    Spacer(Modifier.height(4.dp))
                    // **大きさと容量を並べて出す。** どちらか片方では選べない。
                    Text(
                        "選別で見る絵の大きさ。NAS の原本は 1 枚 6MB あるので、" +
                            "準備のときに一度だけ読んで、この大きさで端末に残します。",
                        fontSize = 11.sp, color = Faint
                    )
                    Spacer(Modifier.height(8.dp))
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        // 実測 1024px で 1 枚 約 80KB。2,000 枚で約 160MB。
                        EdgeChip("標準 1024px", estimate(1024), displayEdge == 1024) {
                            displayEdge = 1024; Prefs.setDisplayEdge(context, 1024)
                        }
                        EdgeChip("大きく 1536px", estimate(1536), displayEdge == 1536) {
                            displayEdge = 1536; Prefs.setDisplayEdge(context, 1536)
                        }
                        EdgeChip(
                            "詳細…", "768–1920px",
                            displayEdge != 1024 && displayEdge != 1536
                        ) { detailing = true }
                    }
                    Spacer(Modifier.height(8.dp))
                    Text(
                        "新しいプロジェクトに適用されます",
                        fontSize = 11.sp, color = Faint
                    )
                }

                Spacer(Modifier.height(16.dp))

                Panel {
                    Text("選別の既定", fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
                    Spacer(Modifier.height(12.dp))

                    Text("一度に見比べる枚数", fontSize = 13.sp)
                    Spacer(Modifier.height(6.dp))
                    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        for (size in 2..10) {
                            SizeDot(size, size == groupSize) {
                                groupSize = size; Prefs.setGroupSize(context, size)
                            }
                        }
                    }
                    Text(
                        "4 枚がおすすめ。多いほど 1 回で絞れますが、1 枚が小さくなります",
                        fontSize = 11.sp, color = Faint,
                        modifier = Modifier.padding(top = 6.dp)
                    )

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

    if (adding || editing != null) {
        NasEditSheet(
            existing = editing,
            onSaved = { adding = false; editing = null; reloads += 1 },
            onRemoved = { adding = false; editing = null; reloads += 1 },
            onDismiss = { adding = false; editing = null }
        )
    }

    // 表示用画像の大きさを細かく決める。**設計の「詳細…（768–1920）」。**
    if (detailing) {
        // 段は 01-方針と状態モデル の 768|1024|1280|1536|1920。
        val steps = listOf(768, 1024, 1280, 1536, 1920)
        var picked by remember { mutableStateOf(displayEdge) }
        AlertDialog(
            onDismissRequest = { detailing = false },
            title = { Text("表示用画像の大きさ") },
            text = {
                Column {
                    Text(
                        "選別で見る絵の長辺です。大きいほどよく見えますが、" +
                            "端末に置く量が増えます。",
                        fontSize = 12.sp, color = Faint
                    )
                    Spacer(Modifier.height(12.dp))
                    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        for (edge in steps) {
                            FilterChip(
                                selected = edge == picked,
                                onClick = { picked = edge },
                                label = { Text("$edge", fontSize = 13.sp) },
                                colors = FilterChipDefaults.filterChipColors(
                                    selectedContainerColor = Lime, selectedLabelColor = Color.Black
                                )
                            )
                        }
                    }
                    Spacer(Modifier.height(10.dp))
                    Text(estimate(picked), fontSize = 12.sp, color = Lime)
                }
            },
            confirmButton = {
                TextButton(onClick = {
                    displayEdge = picked
                    Prefs.setDisplayEdge(context, picked)
                    detailing = false
                }) { Text("これにする") }
            },
            dismissButton = { TextButton(onClick = { detailing = false }) { Text("やめる") } }
        )
    }
}


/**
 * 2,000 枚ぶんのおおよその容量。
 *
 * 実測は 1024px で 1 枚 約 80KB。面積に比例するので、辺の比の 2 乗で見積もる。
 * **数字だけでは選べない**ので、必ず容量を添える。
 */
private fun estimate(edge: Int): String {
    val perPhoto = 80.0 * (edge.toDouble() / 1024).let { it * it }
    val total = (perPhoto * 2000 / 1024).toInt()
    return "2,000 枚で約 ${total}MB"
}

/** 一度に見比べる枚数。丸 1 つ。 */
@Composable
private fun SizeDot(size: Int, selected: Boolean, onClick: () -> Unit) {
    Box(
        Modifier
            .size(36.dp)
            .clip(RoundedCornerShape(18.dp))
            .background(if (selected) Lime else Color.Transparent)
            .then(
                if (selected) Modifier
                else Modifier.border(1.dp, Color(0xFF3A3E47), RoundedCornerShape(18.dp))
            )
            .clickable(onClick = onClick),
        contentAlignment = Alignment.Center
    ) {
        Text(
            "$size", fontSize = 13.sp,
            fontWeight = if (selected) FontWeight.Bold else FontWeight.Normal,
            color = if (selected) Color.Black else Color.White
        )
    }
}

/** 大きさの選択。**容量を添える。** 数字だけでは選べない。 */
@Composable
private fun EdgeChip(label: String, note: String, selected: Boolean, onClick: () -> Unit) {
    FilterChip(
        selected = selected,
        onClick = onClick,
        label = {
            Column {
                Text(label, fontSize = 13.sp)
                Text(
                    note, fontSize = 10.sp,
                    color = if (selected) Color.Black.copy(alpha = 0.7f) else Faint
                )
            }
        },
        colors = FilterChipDefaults.filterChipColors(
            selectedContainerColor = Lime, selectedLabelColor = Color.Black
        )
    )
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
