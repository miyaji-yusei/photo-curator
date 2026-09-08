@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package app.photocurator.next

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * 選別中の「…」。**手を止めずに設定を変えられる場所。**
 *
 * 開始前シートを 2 回目以降に出さないと決めた条件が、この画面。
 * ここが無いと、枚数を変えるのに一度選別をやめる必要がある。
 *
 * 表示枚数は**いまのグループに即座に効く**。足りなければ次のグループから
 * 補い、余れば次のグループへ押し出す。選んだ印はそのまま残す。
 */
@Composable
fun OptionsSheet(
    groupSize: Int,
    groupBursts: Boolean,
    displayEdge: Int,
    /** NAS のときだけ表示用画像の大きさを出す。端末は作らないので意味がない。 */
    showDisplayEdge: Boolean,
    onGroupSize: (Int) -> Unit,
    onGroupBursts: (Boolean) -> Unit,
    onDisplayEdge: (Int) -> Unit,
    onDismiss: () -> Unit
) {
    val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = sheet, containerColor = Surface) {
        Column(Modifier.padding(horizontal = 20.dp).padding(bottom = 24.dp)) {
            Text("選別の設定", fontSize = 17.sp, fontWeight = FontWeight.SemiBold)

            Spacer(Modifier.height(16.dp))
            Text("一度に見比べる枚数", fontSize = 13.sp)
            Text(
                "いまのグループにすぐ効きます。選んだ印は残ります",
                fontSize = 11.sp, color = Faint,
                modifier = Modifier.padding(top = 2.dp, bottom = 8.dp)
            )
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                for (size in 2..10) {
                    Box(
                        Modifier
                            .size(38.dp)
                            .clip(RoundedCornerShape(19.dp))
                            .background(if (size == groupSize) Lime else Color.Transparent)
                            .then(
                                if (size == groupSize) Modifier
                                else Modifier.border(1.dp, Color(0xFF3A3E47), RoundedCornerShape(19.dp))
                            )
                            .clickable { onGroupSize(size) },
                        contentAlignment = Alignment.Center
                    ) {
                        Text(
                            "$size", fontSize = 13.sp,
                            fontWeight = if (size == groupSize) FontWeight.Bold else FontWeight.Normal,
                            color = if (size == groupSize) Color.Black else Color.White
                        )
                    }
                }
            }

            Spacer(Modifier.height(18.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text("似た連写をまとめて 1 枚として見る", fontSize = 13.sp)
                    Text(
                        "切り替えると、まだ見ていない写真だけ組み直します",
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
                    for (edge in listOf(768, 1024, 1280, 1536, 1920)) {
                        FilterChip(
                            selected = edge == displayEdge,
                            onClick = { onDisplayEdge(edge) },
                            label = { Text("$edge", fontSize = 13.sp) },
                            colors = FilterChipDefaults.filterChipColors(
                                selectedContainerColor = Lime, selectedLabelColor = Color.Black
                            )
                        )
                    }
                }
            }
        }
    }
}
