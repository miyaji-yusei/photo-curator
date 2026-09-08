@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package app.photocurator.next

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Folder
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
import kotlinx.coroutines.launch

/**
 * プロジェクトを作る。**出所とフォルダを選ぶ。**
 *
 * 「いま何を見ているか」を常に 1 か所（出所タブ）で示す。
 * タブが下の一覧の出所と 1 対 1 になっていて、**切り替えたら必ず取り直す**。
 * これがずれると、別の場所のフォルダを選んだつもりで作ってしまう。
 */
@Composable
fun CreateSheet(onCreated: (Project) -> Unit, onDismiss: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    // いまは「この端末」だけ。NAS のタブは繋げられるようになってから足す。
    var tab by remember { mutableStateOf("album") }
    var albums by remember { mutableStateOf<List<Album>>(emptyList()) }
    var taken by remember { mutableStateOf<Set<String>>(emptySet()) }
    var note by remember { mutableStateOf("読み込み中…") }
    var chosen by remember { mutableStateOf<Album?>(null) }
    var name by remember { mutableStateOf("") }

    // **タブが変わったら必ず取り直す。** 前のタブの一覧が残っていると、
    // 見出しと中身が食い違う。
    LaunchedEffect(tab) {
        albums = emptyList()
        chosen = null
        note = "読み込み中…"
        if (tab == "album") {
            albums = Photos.albums(context)
            taken = Projects.all(context)
                .filter { it.source.kind == "album" }
                .map { it.source.key }
                .toSet()
            note = if (albums.isEmpty()) "この端末に写真のフォルダがありません" else ""
        } else {
            note = "NAS はまだ繋げません"
        }
    }

    // **主ボタンを畳んだ高さの外に置かない。** 半開きだと「作成して準備を始める」
    // が見えず、選んだのに次へ進めない画面になる。
    val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheet,
        containerColor = Surface
    ) {
        Column(Modifier.padding(horizontal = 16.dp).padding(bottom = 16.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("プロジェクトを作成", fontSize = 18.sp, fontWeight = FontWeight.SemiBold)
                Spacer(Modifier.weight(1f))
                IconButton(onClick = onDismiss) { Icon(Icons.Filled.Close, "閉じる") }
            }

            // ---- 出所タブ ----
            Row(
                Modifier.padding(top = 4.dp, bottom = 12.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp)
            ) {
                FilterChip(
                    selected = tab == "album",
                    onClick = { tab = "album" },
                    label = { Text("この端末", fontSize = 13.sp) },
                    colors = FilterChipDefaults.filterChipColors(
                        selectedContainerColor = Lime, selectedLabelColor = Color.Black
                    )
                )
                FilterChip(
                    selected = tab == "nas",
                    onClick = { tab = "nas" },
                    label = { Text("NAS を追加", fontSize = 13.sp) },
                    colors = FilterChipDefaults.filterChipColors(
                        selectedContainerColor = Lime, selectedLabelColor = Color.Black
                    )
                )
            }

            Row(Modifier.heightIn(max = 380.dp)) {
                // ---- 左: フォルダ一覧 ----
                Column(Modifier.weight(1f)) {
                    Text(
                        if (tab == "album") "この端末のフォルダ · ${albums.size} 件" else "NAS",
                        fontSize = 12.sp, color = Faint,
                        modifier = Modifier.padding(bottom = 6.dp)
                    )
                    if (note.isNotEmpty()) {
                        Text(
                            note, fontSize = 13.sp, color = Faint,
                            modifier = Modifier.padding(vertical = 24.dp)
                        )
                    }
                    LazyColumn {
                        items(albums, key = { it.id }) { album ->
                            val here = chosen?.id == album.id
                            val already = album.id in taken
                            Row(
                                Modifier
                                    .fillMaxWidth()
                                    .clip(RoundedCornerShape(10.dp))
                                    .then(
                                        if (here) Modifier
                                            .background(Color(0x24D6FF73))
                                            .border(1.dp, Lime, RoundedCornerShape(10.dp))
                                        else Modifier
                                    )
                                    .clickable {
                                        chosen = album
                                        // 名前の既定はフォルダ名。**ID は入れない。**
                                        name = album.name
                                    }
                                    .padding(horizontal = 10.dp, vertical = 12.dp),
                                verticalAlignment = Alignment.CenterVertically
                            ) {
                                Icon(Icons.Filled.Folder, null, Modifier.size(18.dp), tint = Faint)
                                Spacer(Modifier.width(10.dp))
                                Text(album.name, fontSize = 14.sp, modifier = Modifier.weight(1f))
                                if (already) {
                                    Text("作成済み", fontSize = 12.sp, color = Faint)
                                } else {
                                    Text("${album.count} 枚", fontSize = 12.sp, color = Faint)
                                }
                                if (here) {
                                    Spacer(Modifier.width(8.dp))
                                    Icon(Icons.Filled.Check, null, Modifier.size(16.dp), tint = Lime)
                                }
                            }
                        }
                    }
                }

                Spacer(Modifier.width(16.dp))

                // ---- 右: 名前と出所の確認 ----
                Column(Modifier.width(260.dp)) {
                    OutlinedTextField(
                        value = name,
                        onValueChange = { name = it },
                        singleLine = true,
                        label = { Text("プロジェクト名") },
                        enabled = chosen != null,
                        modifier = Modifier.fillMaxWidth()
                    )
                    Text(
                        "フォルダ名を使います。あとで変えられます",
                        fontSize = 11.sp, color = Faint,
                        modifier = Modifier.padding(top = 4.dp, bottom = 14.dp)
                    )
                    chosen?.let { album ->
                        Confirm("出所", "この端末")
                        Confirm("フォルダ", album.relativeDir.ifBlank { album.name })
                        Confirm("写真", "${album.count} 枚")
                        Spacer(Modifier.height(12.dp))
                        Row(verticalAlignment = Alignment.Top) {
                            Icon(Icons.Filled.Lock, null, Modifier.size(14.dp), tint = Faint)
                            Spacer(Modifier.width(6.dp))
                            Text(
                                "原本は読むだけ。星やまとめ方はこのアプリの中に保存します",
                                fontSize = 11.sp, color = Faint
                            )
                        }
                    }
                }
            }

            Row(
                Modifier.fillMaxWidth().padding(top = 16.dp),
                horizontalArrangement = Arrangement.End,
                verticalAlignment = Alignment.CenterVertically
            ) {
                TextButton(onClick = onDismiss) { Text("キャンセル") }
                Spacer(Modifier.width(8.dp))
                Button(
                    onClick = {
                        val album = chosen ?: return@Button
                        val wanted = name.trim().ifEmpty { album.name }
                        scope.launch {
                            val project = Projects.add(
                                context, wanted,
                                Source(
                                    kind = "album",
                                    // **人の言葉。** 生の ID は技術情報だけに出す。
                                    label = "この端末・アルバム「${album.name}」",
                                    key = album.id
                                )
                            )
                            onCreated(project)
                        }
                    },
                    enabled = chosen != null,
                    shape = RoundedCornerShape(50)
                ) {
                    Text("作成して準備を始める", fontWeight = FontWeight.Bold)
                }
            }
        }
    }
}

@Composable
private fun Confirm(label: String, value: String) {
    Row(Modifier.fillMaxWidth().padding(vertical = 3.dp)) {
        Text(label, fontSize = 12.sp, color = Faint, modifier = Modifier.width(64.dp))
        Text(value, fontSize = 12.sp)
    }
}
