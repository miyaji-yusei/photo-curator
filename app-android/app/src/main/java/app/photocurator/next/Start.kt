@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package app.photocurator.next

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.PlayArrow
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
 * 選別を始める前の確認。
 *
 * **「毎回確認」が off なら 2 回目以降は出さない。** 選別中に `…` から
 * 変えられるので、始めるたびに同じ問いを置くと邪魔になるだけ。
 * ただし**初回は必ず出す**。何がどう始まるのかを一度も見ないまま
 * 数百枚の判断に入らせない。
 */
@Composable
fun StartSheet(
    project: Project,
    photoCount: Int,
    /** 表示用画像ができている枚数。全部できていれば photoCount と同じ。 */
    readyCount: Int = photoCount,
    onStart: () -> Unit,
    onDismiss: () -> Unit
) {
    val context = LocalContext.current
    var groupSize by remember { mutableStateOf(Prefs.groupSize(context)) }
    var groupBursts by remember { mutableStateOf(Prefs.groupBursts(context)) }
    // 既定 on。**設定の「毎回確認」とは裏返し**なので、ここで on＝もう出さない。
    var hideNext by remember { mutableStateOf(!Prefs.askBeforeStart(context)) }

    val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = sheet, containerColor = Surface) {
        Column(Modifier.padding(horizontal = 20.dp).padding(bottom = 20.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("選別を始める前に", fontSize = 18.sp, fontWeight = FontWeight.SemiBold)
                Spacer(Modifier.weight(1f))
                Text("${project.name} · $photoCount 枚", fontSize = 12.sp, color = Faint)
            }

            Spacer(Modifier.height(18.dp))

            Row {
                // ---- 左: 一度に見比べる枚数 ----
                Column(Modifier.weight(1f)) {
                    Text("一度に見比べる枚数", fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                    Spacer(Modifier.height(8.dp))
                    // 2〜10。**4 をおすすめとして真ん中に据える。**
                    // 多いほど 1 回で絞れるが、1 枚が小さくなる。
                    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        for (size in 2..10) {
                            SizeCard(
                                number = "$size",
                                note = when (size) {
                                    2 -> "大きく"
                                    4 -> "おすすめ"
                                    10 -> "小さく"
                                    else -> ""
                                },
                                selected = groupSize == size
                            ) { groupSize = size }
                        }
                    }
                    // **回数で言う。** 何回タップすることになるのかが、
                    // 枚数を選ぶときに一番知りたいこと。
                    val rounds = if (groupSize > 0) (photoCount + groupSize - 1) / groupSize else 0
                    Text(
                        "$groupSize 枚なら約 $rounds 回で ROUND 1 が終わります。" +
                            "選別中に … から変えられます",
                        fontSize = 11.sp, color = Faint,
                        modifier = Modifier.padding(top = 10.dp)
                    )
                }

                Spacer(Modifier.width(20.dp))

                // ---- 右: まとめ方と次回の扱い ----
                Column(Modifier.width(300.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Column(Modifier.weight(1f)) {
                            Text("似た連写をまとめて 1 枚として見る", fontSize = 13.sp)
                            Text(
                                "同じ瞬間を何度も見ずに済みます",
                                fontSize = 11.sp, color = Faint
                            )
                        }
                        Switch(
                            checked = groupBursts,
                            onCheckedChange = { groupBursts = it },
                            colors = SwitchDefaults.colors(
                                checkedThumbColor = Color.Black, checkedTrackColor = Lime
                            )
                        )
                    }
                    Spacer(Modifier.height(10.dp))
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text("次回からこの画面を出さない", fontSize = 13.sp, modifier = Modifier.weight(1f))
                        Switch(
                            checked = hideNext,
                            onCheckedChange = { hideNext = it },
                            colors = SwitchDefaults.colors(
                                checkedThumbColor = Color.Black, checkedTrackColor = Lime
                            )
                        )
                    }
                }
            }

            // **まだ作っている途中でも始められる。** そう言っておく。
            if (readyCount < photoCount) {
                Spacer(Modifier.height(14.dp))
                Text(
                    "準備が終わった $readyCount 枚から始めます。残りは順次追加します",
                    fontSize = 12.sp, color = Faint
                )
            }

            Row(
                Modifier.fillMaxWidth().padding(top = 20.dp),
                horizontalArrangement = Arrangement.End,
                verticalAlignment = Alignment.CenterVertically
            ) {
                TextButton(onClick = onDismiss) { Text("キャンセル") }
                Spacer(Modifier.width(8.dp))
                Button(
                    onClick = {
                        Prefs.setGroupSize(context, groupSize)
                        Prefs.setGroupBursts(context, groupBursts)
                        Prefs.setAskBeforeStart(context, !hideNext)
                        onStart()
                    },
                    shape = RoundedCornerShape(50)
                ) {
                    Icon(Icons.Filled.PlayArrow, null, Modifier.size(18.dp))
                    Spacer(Modifier.width(6.dp))
                    Text("選別を開始", fontWeight = FontWeight.Bold)
                }
            }
        }
    }
}

@Composable
private fun SizeCard(number: String, note: String, selected: Boolean, onClick: () -> Unit) {
    Column(
        Modifier
            .size(width = 60.dp, height = 66.dp)
            .clip(RoundedCornerShape(12.dp))
            .background(if (selected) Lime else Tile)
            .then(if (selected) Modifier else Modifier.border(1.dp, Color(0xFF24272D), RoundedCornerShape(12.dp)))
            .clickable(onClick = onClick)
            .padding(8.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally
    ) {
        Text(
            number,
            fontSize = 20.sp, fontWeight = FontWeight.Bold,
            color = if (selected) Color.Black else Color.White
        )
        if (note.isNotEmpty()) {
            Text(
                note,
                fontSize = 9.sp,
                color = if (selected) Color.Black else Faint
            )
        }
    }
}
