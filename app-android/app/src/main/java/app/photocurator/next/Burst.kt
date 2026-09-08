@file:OptIn(
    androidx.compose.material3.ExperimentalMaterial3Api::class,
    androidx.compose.foundation.ExperimentalFoundationApi::class
)

package app.photocurator.next

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.DragIndicator
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import coil.compose.AsyncImage
import coil.request.ImageRequest

/**
 * まとまり編集シート。**まとまり＝時間軸上の区間**として見せる。
 *
 * 操作は 1 種類しかない: **線を動かすか、写真をタップするか。**
 * 「外す」「足す」「切る」の専用ボタンは置かない。1 枚を線 2 本で挟めば
 * それが「外す」になり、外側の線を外へ動かせばそれが「足す」になる。
 *
 * 色は 3 色を巡回させる。隣り合うまとまりが同じ色だと、どこで切れているのか
 * 読めない。単独の写真は色を持たず、巡回も進めない。
 *
 * **確定するまでデータは変えない。** 下に「元の 5 枚 → A 3 枚 ＋ B 2 枚」と
 * 結果を先に文で出してから押させる。
 */
@Composable
fun BurstEditSheet(
    /** 編集の対象になる写真（撮影順）。まとまりの前後の単独写真も含む。 */
    photos: List<Photo>,
    /** いまのまとまり。区間の開始位置（photos の添字）の集合で表す。 */
    initialCuts: Set<Int>,
    /** いまの代表（photos の添字）。区間ごとに 1 つ。 */
    initialLeaders: Map<Int, Int>,
    displayEdge: Int,
    onApply: (cuts: Set<Int>, leaders: Map<Int, Int>) -> Unit,
    onZoom: (Photo) -> Unit,
    onDismiss: () -> Unit
) {
    // 区切りの位置。**添字 i は「i-1 と i のあいだで切る」の意味。**
    // 0 は必ず区切り（先頭）なので持たない。
    var cuts by remember { mutableStateOf(initialCuts) }
    var leaders by remember { mutableStateOf(initialLeaders) }

    val density = LocalDensity.current
    val tileWidth = 180.dp
    val gap = 8.dp
    val stride = with(density) { (tileWidth + gap).toPx() }

    /** 区間の開始添字を並べる。 */
    val starts = remember(cuts, photos) {
        (listOf(0) + cuts.filter { it in 1 until photos.size }).distinct().sorted()
    }

    /** 添字 → その写真が属する区間の開始添字。 */
    fun startOf(at: Int): Int = starts.last { it <= at }

    /** 区間の大きさ。 */
    fun sizeOf(start: Int): Int {
        val next = starts.firstOrNull { it > start } ?: photos.size
        return next - start
    }

    // 色は**まとまりの並び順**で決める（保存しない）。単独は色を持たない。
    val palette = listOf(Color(0xFFD6FF73), Color(0xFFA7C8FF), Color(0xFFFFB3E6))
    val colorOf = remember(starts, photos) {
        val out = HashMap<Int, Color>()
        var turn = 0
        for (start in starts) {
            val next = starts.firstOrNull { it > start } ?: photos.size
            if (next - start > 1) {
                out[start] = palette[turn % palette.size]
                // **単独が間に入っても巡回は進めない。**
                turn += 1
            }
        }
        out
    }

    val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = sheet, containerColor = Surface) {
        Column(Modifier.padding(horizontal = 16.dp).padding(bottom = 16.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("連写のまとまりを編集", fontSize = 17.sp, fontWeight = FontWeight.SemiBold)
                Spacer(Modifier.weight(1f))
                Text(
                    "線を動かして分ける／つなげる · 写真をタップで代表 · 横にスクロール",
                    fontSize = 11.sp, color = Faint
                )
            }

            Spacer(Modifier.height(14.dp))

            Row(
                Modifier.horizontalScroll(rememberScrollState()).height(140.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                for (at in photos.indices) {
                    val start = startOf(at)
                    val tint = colorOf[start]
                    val isLeader = (leaders[start] ?: start) == at
                    val grouped = sizeOf(start) > 1

                    // ---- 写真のあいだの区切り ----
                    if (at > 0) {
                        Divider(
                            standing = at in cuts,
                            tint = tint ?: Color(0xFF5F6572),
                            onTap = {
                                // **隙間をタップ＝ここで分ける／つなげる。**
                                cuts = if (at in cuts) cuts - at else cuts + at
                            },
                            onDrag = { toRight ->
                                // 線を隣まで動かす。重ねれば消える＝つながる。
                                val target = if (toRight) at + 1 else at - 1
                                if (at in cuts && target in 1 until photos.size) {
                                    cuts = cuts - at + target
                                } else if (at in cuts) {
                                    cuts = cuts - at
                                }
                            },
                            stride = stride
                        )
                    }

                    // ---- 写真 ----
                    Box(
                        Modifier
                            .width(tileWidth)
                            .height(120.dp)
                            .clip(RoundedCornerShape(8.dp))
                            .background(if (grouped && tint != null) tint.copy(alpha = 0.10f) else Tile)
                            .then(
                                if (isLeader && grouped && tint != null)
                                    Modifier.border(4.dp, tint, RoundedCornerShape(8.dp))
                                else Modifier
                            )
                            .combinedClickable(
                                // **まとまり内の写真をタップ＝代表がそこへ移る。**
                                onClick = { if (grouped) leaders = leaders + (start to at) },
                                onLongClick = { onZoom(photos[at]) }
                            )
                    ) {
                        AsyncImage(
                            model = ImageRequest.Builder(LocalContext.current)
                                .data(photos[at].displayModel(displayEdge)).size(360).build(),
                            contentDescription = photos[at].name,
                            imageLoader = Images.loader(LocalContext.current),
                            contentScale = ContentScale.Crop,
                            // まとまりの外の写真は薄く。**触れることは触れる。**
                            modifier = Modifier.fillMaxSize()
                                .then(
                                    // まとまりの外は薄く。**触れることは触れる。**
                                    if (grouped) Modifier else Modifier.graphicsLayer { alpha = 0.45f }
                                )
                        )
                        if (isLeader && grouped) {
                            Text(
                                "代表",
                                fontSize = 11.sp, color = Color.Black,
                                modifier = Modifier
                                    .padding(6.dp)
                                    .clip(RoundedCornerShape(5.dp))
                                    .background(tint ?: Lime)
                                    .padding(horizontal = 7.dp, vertical = 2.dp)
                            )
                        }
                    }
                }
            }

            Spacer(Modifier.height(12.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                // **単独の写真は名前を消費しない。** A・B・C と続けて出す。
                // 添字をそのまま使うと「B・C・E」のように飛んで、
                // どこかに D があるのに見えていないように読める。
                var letter = 0
                for (start in starts) {
                    val size = sizeOf(start)
                    if (size <= 1) continue
                    Legend(colorOf[start] ?: Lime, "まとまり ${'A' + letter} · $size 枚")
                    letter += 1
                }
                Legend(Color(0xFF5F6572), "灰色の線 ＝ まとまりの外側")
            }

            Spacer(Modifier.height(14.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                // **結果を先に文で。** 押す前に何が起きるか分かるように。
                val groups = starts.count { sizeOf(it) > 1 }
                val singles = starts.count { sizeOf(it) == 1 }
                Text(
                    "元の ${photos.size} 枚 → まとまり $groups 組 ＋ 単独 $singles 枚",
                    fontSize = 12.sp, color = Faint, modifier = Modifier.weight(1f)
                )
                TextButton(onClick = onDismiss) { Text("キャンセル") }
                Spacer(Modifier.width(8.dp))
                Button(
                    onClick = { onApply(cuts, leaders) },
                    shape = RoundedCornerShape(50)
                ) { Text("この分け方にする", fontWeight = FontWeight.Bold) }
            }
        }
    }
}

/**
 * 写真のあいだの区切り。**立っていれば色付き 4px、無ければ灰色の細い線。**
 * 44px の丸ハンドルで掴めるようにする（指で触れる大きさ）。
 */
@Composable
private fun Divider(
    standing: Boolean,
    tint: Color,
    onTap: () -> Unit,
    onDrag: (toRight: Boolean) -> Unit,
    stride: Float
) {
    Box(
        Modifier.width(28.dp).height(140.dp).clickable(onClick = onTap),
        contentAlignment = Alignment.Center
    ) {
        Box(
            Modifier
                .width(if (standing) 4.dp else 1.dp)
                .fillMaxHeight()
                .background(if (standing) tint else Color(0xFF5F6572))
        )
        if (standing) {
            var moved by remember { mutableStateOf(0f) }
            Box(
                Modifier
                    .size(44.dp)
                    .clip(RoundedCornerShape(22.dp))
                    .background(tint)
                    .pointerInput(stride) {
                        detectHorizontalDragGestures(
                            onDragEnd = {
                                // 隣の位置まで動かしたら、そこへ移す。
                                if (kotlin.math.abs(moved) > stride * 0.5f) onDrag(moved > 0)
                                moved = 0f
                            }
                        ) { _, amount -> moved += amount }
                    },
                contentAlignment = Alignment.Center
            ) {
                Icon(Icons.Filled.DragIndicator, "動かす", Modifier.size(22.dp), tint = Color.Black)
            }
        }
    }
}

@Composable
private fun Legend(tint: Color, label: String) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.size(9.dp).clip(RoundedCornerShape(5.dp)).background(tint))
        Spacer(Modifier.width(5.dp))
        Text(label, fontSize = 11.sp, color = Faint)
    }
}
