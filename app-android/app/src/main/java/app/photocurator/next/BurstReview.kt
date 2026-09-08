package app.photocurator.next

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * 連写の中身を選別する。**まとまりで 1 つに畳んだ中を、あとから開き直す場所。**
 *
 * 選別では代表しか見えない。代表が通ると仲間も同じ星をもらうので、
 * 「この組の中では 3 枚目が一番いい」という違いは残らない。ここはその
 * 差をつけ直すためだけの画面。
 *
 * **ラウンドにしない。** 組の中身はもともと数枚しかなく、勝ち抜きにすると
 * 同じ写真を何度も見ることになる。**全部を 1 画面に並べて、良いものを選ぶ。**
 * 一度に見比べる枚数より多ければ、下へ送って見る。
 *
 * 星の決まり:
 * - 選んだ写真は **いまの星 ＋ 1**（★5 が上限）
 * - 選ばなかった写真は **いまの星のまま**
 * - **決めるまでデータは変えない。** 何が起きるかを先に文で出す
 */
@Composable
fun BurstReviewScreen(
    /** まとまりの中身（撮影順）。代表を含む。 */
    photos: List<Photo>,
    /** このまとまりが今持っている星。ここを土台に上げる。 */
    baseStar: Int,
    displayEdge: Int,
    /** 一度に見比べる枚数。**枠の大きさを決めるだけ**で、区切りではない。 */
    groupSize: Int,
    /** 相対パス → 新しい星。押されたときだけ呼ぶ。 */
    onApply: (Map<String, Int>) -> Unit,
    onZoom: (Int) -> Unit,
    onBack: () -> Unit
) {
    var picked by remember(photos) { mutableStateOf<Set<String>>(emptySet()) }
    var stageSize by remember { mutableStateOf(0 to 0) }
    val density = LocalDensity.current

    val landscape = stageSize.first >= stageSize.second
    val (rows, cols) = gridFor(groupSize.coerceIn(2, 10), landscape)
    // 1 枚の高さは「一度に見比べる枚数」で決める。**はみ出した分は下へ送る。**
    val tileHeight = with(density) {
        if (stageSize.second > 0) (stageSize.second / rows).toDp() else 200.dp
    }
    val next = photos.associate {
        it.relativePath to
            (if (it.relativePath in picked) baseStar + 1 else baseStar).coerceIn(0, 5)
    }

    Column(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing)) {
        Row(
            Modifier.fillMaxWidth().height(64.dp).padding(end = 16.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            IconButton(onClick = onBack) { Icon(Icons.Filled.ArrowBack, "戻る") }
            Column(Modifier.weight(1f)) {
                Text("連写の中身を選別", fontSize = 13.sp, color = Lime)
                Text(
                    "${photos.size} 枚 · いまは全部 ★$baseStar",
                    fontSize = 12.sp, color = Faint
                )
            }
            // **押す前に何が起きるかを言う。**
            Text(
                if (picked.isEmpty()) "選ぶと ★${(baseStar + 1).coerceAtMost(5)} に上がります"
                else "${picked.size} 枚が ★${(baseStar + 1).coerceAtMost(5)} に上がります",
                fontSize = 12.sp, color = Faint,
                modifier = Modifier.padding(end = 12.dp)
            )
            Button(
                onClick = { onApply(next) },
                enabled = picked.isNotEmpty(),
                shape = RoundedCornerShape(50)
            ) { Text("この結果にする", fontWeight = FontWeight.Bold, fontSize = 13.sp) }
        }

        LazyVerticalGrid(
            columns = GridCells.Fixed(cols),
            modifier = Modifier
                .weight(1f)
                .padding(6.dp)
                .onSizeChanged { stageSize = it.width to it.height },
            verticalArrangement = Arrangement.spacedBy(6.dp),
            horizontalArrangement = Arrangement.spacedBy(6.dp)
        ) {
            items(photos, key = { it.id }) { photo ->
                Box(Modifier.height(tileHeight)) {
                    Tile(
                        number = photos.indexOf(photo) + 1,
                        photo = photo,
                        picked = photo.relativePath in picked,
                        // ここは畳まないので、1 枚は必ず 1 枚。
                        stands = 1,
                        displayEdge = displayEdge,
                        onZoom = { onZoom(photos.indexOf(photo)) },
                        onHold = { onZoom(photos.indexOf(photo)) },
                        onOpenBurst = {},
                        // **ここは 1 タップで確定しない。** 組の中は見比べて
                        // 選ぶ場所なので、選んでから確定する。
                        onTap = {
                            picked = if (photo.relativePath in picked) {
                                picked - photo.relativePath
                            } else picked + photo.relativePath
                        }
                    )
                }
            }
        }

        Text(
            "選んだ写真だけ ★${(baseStar + 1).coerceAtMost(5)} に上がります。" +
                "選ばなければ ★$baseStar のまま。一度に見比べる枚数より多い分は下へ送って見られます",
            fontSize = 11.sp, color = Faint,
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp)
        )
    }
}
