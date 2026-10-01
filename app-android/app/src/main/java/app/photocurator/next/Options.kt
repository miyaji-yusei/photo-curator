@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package app.photocurator.next

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
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
 * 選別中の「…」。**手を止めずに設定を変えられる場所。**
 *
 * 開始前シートを 2 回目以降に出さないと決めた条件が、この画面。
 * ここが無いと、枚数を変えるのに一度選別をやめる必要がある。
 *
 * **方式と枚数は、その場では反映しない。** PC・Web の GroupSizeDialog と同じく、
 * 選んでおいて「この方式にする」「この枚数にする」で反映する（キャンセルで何も変えない）。
 * 反映すると、いまのグループに効く。足りなければ次のグループから補い、
 * 余れば次のグループへ押し出す。選んだ印はそのまま残す。
 * 連写をまとめるスイッチと表示用画像の大きさは、今までどおりその場で反映する。
 */
@Composable
fun OptionsSheet(
    groupSize: Int,
    groupBursts: Boolean,
    displayEdge: Int,
    /** 網越しのときだけ表示用画像の大きさを出す。端末は作らないので意味がない。 */
    showDisplayEdge: Boolean,
    /** 出せる長辺の上限。**0 は上限なし。** これを超える大きさは非活性（設計 08 章 8.5）。 */
    maxEdge: Int = 0,
    /** 確定ボタンで反映する枚数（1 はスライドショー）。 */
    onApplyGroupSize: (Int) -> Unit,
    onGroupBursts: (Boolean) -> Unit,
    onDisplayEdge: (Int) -> Unit,
    onDismiss: () -> Unit
) {
    val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val context = LocalContext.current
    // 選んでいる途中の方式と枚数。確定するまで core にも設定にも書かない。
    var pending by remember { mutableStateOf(PendingMethod.from(groupSize, Prefs.tournamentSize(context))) }
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheet,
        containerColor = Surface,
        // **下の帯まで自分の色で塗る。** 既定だとナビゲーションバーのところが
        // 白く残り、一番下のボタンに被る。
        contentWindowInsets = { WindowInsets(0) }
    ) {
        Column(
            Modifier
                .padding(horizontal = 20.dp)
                .padding(bottom = 24.dp)
                .verticalScroll(rememberScrollState())
        ) {
            Text("選別の設定", fontSize = 17.sp, fontWeight = FontWeight.SemiBold)

            // ---- 方式。**スライドショーは 1 グループ 1 枚**（枚数の設定と同じ仕組み）。
            // 中断して再開したあとでも、ここから選び直せる。 ----
            Spacer(Modifier.height(16.dp))
            Text("選別の方式", fontSize = 13.sp)
            Spacer(Modifier.height(8.dp))
            MethodSegment(pending.method, onChange = { pending = pending.withMethod(it) })

            Spacer(Modifier.height(16.dp))
            if (pending.method == Method.Slideshow) {
                Text(SLIDESHOW_NOTE, fontSize = 11.sp, color = Faint)
            } else {
                Text("1 グループの表示枚数", fontSize = 13.sp)
                Text(
                    "まだ見ていない写真だけを詰め直します。ここまでの選択と「1つ戻す」の履歴はそのまま残ります",
                    fontSize = 11.sp, color = Faint,
                    modifier = Modifier.padding(top = 2.dp, bottom = 8.dp)
                )
                Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    for (size in 2..10) {
                        val selected = size == pending.tournamentSize
                        Box(
                            Modifier
                                .size(38.dp)
                                .clip(RoundedCornerShape(19.dp))
                                .background(if (selected) Lime else Color.Transparent)
                                .then(
                                    if (selected) Modifier
                                    else Modifier.border(1.dp, Color(0xFF3A3E47), RoundedCornerShape(19.dp))
                                )
                                .clickable { pending = pending.withSize(size) },
                            contentAlignment = Alignment.Center
                        ) {
                            Text(
                                "$size", fontSize = 13.sp,
                                fontWeight = if (selected) FontWeight.Bold else FontWeight.Normal,
                                color = if (selected) Color.Black else Color.White
                            )
                        }
                    }
                }
            }

            Spacer(Modifier.height(18.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text("似た連写をまとめて 1 枚として見る", fontSize = 13.sp)
                    Text(
                        "切り替えるとすぐ今の組に効きます。済んだ組はそのまま残ります",
                        fontSize = 11.sp, color = Faint
                    )
                }
                Switch(
                    checked = groupBursts,
                    onCheckedChange = onGroupBursts,
                    colors = SwitchDefaults.colors(
                        checkedThumbColor = Color.Black, checkedTrackColor = Lime
                    )
                )
            }

            if (showDisplayEdge) {
                Spacer(Modifier.height(18.dp))
                Text("表示用画像の大きさ", fontSize = 13.sp)
                Text(
                    "変えると、まだ作っていない大きさで作り直します",
                    fontSize = 11.sp, color = Faint,
                    modifier = Modifier.padding(top = 2.dp, bottom = 8.dp)
                )
                Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    for (edge in Prefs.EDGES) {
                        FilterChip(
                            selected = edge == displayEdge,
                            onClick = { onDisplayEdge(edge) },
                            enabled = Prefs.edgeAllowed(edge, maxEdge),
                            label = { Text("$edge", fontSize = 13.sp) },
                            colors = FilterChipDefaults.filterChipColors(
                                selectedContainerColor = Lime, selectedLabelColor = Color.Black
                            )
                        )
                    }
                }
            }

            // ---- 確定。方式と枚数だけが、ここで反映される。キャンセルは何も変えずに閉じる。 ----
            Row(
                Modifier.fillMaxWidth().padding(top = 20.dp),
                horizontalArrangement = Arrangement.End,
                verticalAlignment = Alignment.CenterVertically
            ) {
                TextButton(onClick = onDismiss) { Text("キャンセル") }
                Spacer(Modifier.width(8.dp))
                Button(
                    onClick = {
                        onApplyGroupSize(pending.size)
                        onDismiss()
                    },
                    shape = RoundedCornerShape(50)
                ) { Text(pending.confirmLabel, fontWeight = FontWeight.Bold) }
            }
        }
    }
}
