package app.photocurator.next

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import coil.compose.AsyncImage
import coil.request.ImageRequest
import uniffi.photo_curator_core.BurstThreshold
import uniffi.photo_curator_core.PhotoRef
import uniffi.photo_curator_core.groupBursts

/**
 * まとまりの確認。**この基準で何組になるかを、始める前に見せる。**
 *
 * 1 行目が答え（「645 枚 → 599 グループ」）。スライダーで緩め・厳しめを
 * 動かすと即座に変わる。**閾値の数字は出さない。** 距離 9 と言われても
 * 判断できないし、判断させたいのは「まとまり方が納得できるか」だけ。
 *
 * ここでの調整は主ボタンを押すまで確定しない。
 */
@Composable
fun PreviewScreen(
    project: Project,
    refs: List<PhotoRef>,
    byPath: Map<String, Photo>,
    learned: Int,
    groupSize: Int,
    onStart: (Int) -> Unit,
    onBack: () -> Unit
) {
    // スライダーは距離そのもの。**目盛りの数字は見せない。**
    var distance by remember { mutableStateOf(learned.coerceIn(2, 24)) }

    val groups = remember(distance, refs) {
        if (refs.isEmpty()) emptyList()
        else groupBursts(
            refs,
            BurstThreshold(windowMs = 4000, distance = distance.toUInt(), dHashVersion = Analyse.VERSION),
            emptyList()
        )
    }
    val bursts = groups.count { it.members.size > 1 }
    val inBursts = groups.filter { it.members.size > 1 }.sumOf { it.members.size }
    val singles = groups.size - bursts
    val turns = if (groupSize > 0) (groups.size + groupSize - 1) / groupSize else 0

    Column(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing)) {
        Row(
            Modifier.fillMaxWidth().height(64.dp).padding(end = 16.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            IconButton(onClick = onBack) { Icon(Icons.Filled.ArrowBack, "戻る") }
            Text("連写のまとまり", fontSize = 16.sp, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.weight(1f))
            Button(onClick = { onStart(distance) }, shape = RoundedCornerShape(50)) {
                Icon(Icons.Filled.PlayArrow, null, Modifier.size(18.dp))
                Spacer(Modifier.width(6.dp))
                Text("この基準で選別を開始", fontWeight = FontWeight.Bold, fontSize = 14.sp)
            }
        }

        Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
            // ---- 左: 1 行目が答え ----
            Column(Modifier.weight(1f)) {
                Text(
                    "${refs.size} 枚 → ${groups.size} グループ",
                    fontSize = 30.sp, fontWeight = FontWeight.Bold
                )
                Text(
                    "連写 $bursts 組（$inBursts 枚）＋ 単独 $singles 枚" +
                        " / ROUND 1 は約 $turns 回で終わります",
                    fontSize = 12.sp, color = Faint,
                    modifier = Modifier.padding(top = 4.dp)
                )
            }

            // ---- 右: 緩く ←→ 厳しく ----
            Column(Modifier.width(320.dp)) {
                Row(Modifier.fillMaxWidth()) {
                    Text("緩く", fontSize = 11.sp, color = Faint)
                    Spacer(Modifier.weight(1f))
                    Text("厳しく", fontSize = 11.sp, color = Faint)
                }
                Slider(
                    // 右へ行くほど厳しく（＝距離が小さく）なるよう裏返す。
                    // 「厳しくすると増える」が直感に合う向き。
                    value = (26 - distance).toFloat(),
                    onValueChange = { distance = (26 - it).toInt().coerceIn(2, 24) },
                    valueRange = 2f..24f,
                    colors = SliderDefaults.colors(
                        thumbColor = Lime, activeTrackColor = Lime,
                        inactiveTrackColor = Color(0xFF24272D)
                    )
                )
                Row(Modifier.fillMaxWidth()) {
                    Text("少ないグループ", fontSize = 11.sp, color = Faint)
                    Spacer(Modifier.weight(1f))
                    Text("多いグループ", fontSize = 11.sp, color = Faint)
                }
            }
        }

        Spacer(Modifier.height(12.dp))

        // ---- 一覧: 撮影順。まとまりは白 1px 枠（選別画面と同じ記号） ----
        val flat = remember(groups) {
            groups.flatMap { group ->
                group.members.mapIndexed { index, path ->
                    Triple(path, group.members.size, index == 0)
                }
            }
        }
        LazyVerticalGrid(
            columns = GridCells.Adaptive(minSize = 84.dp),
            modifier = Modifier.weight(1f),
            contentPadding = PaddingValues(horizontal = 14.dp)
        ) {
            items(flat, key = { it.first }) { (path, size, first) ->
                val photo = byPath[path]
                Box(
                    Modifier
                        .padding(2.dp)
                        .aspectRatio(4f / 3f)
                        .clip(RoundedCornerShape(4.dp))
                        .background(Tile)
                        .then(
                            if (size > 1) Modifier.border(1.dp, Color.White, RoundedCornerShape(4.dp))
                            else Modifier
                        )
                ) {
                    if (photo != null) {
                        AsyncImage(
                            model = ImageRequest.Builder(LocalContext.current)
                                .data(photo.uri).size(160).build(),
                            contentDescription = photo.name,
                            contentScale = ContentScale.Crop,
                            modifier = Modifier.fillMaxSize()
                        )
                    }
                    // まとまりの先頭にだけ枚数を出す。全部に出すとうるさい。
                    if (size > 1 && first) {
                        Text(
                            "⧉$size",
                            fontSize = 10.sp, color = Color.White,
                            modifier = Modifier
                                .padding(3.dp)
                                .clip(RoundedCornerShape(3.dp))
                                .background(Color(0xB3101114))
                                .padding(horizontal = 4.dp, vertical = 1.dp)
                        )
                    }
                }
            }
        }

        Text(
            "白い枠が 1 つのまとまり。選別では代表 1 枚だけが出ます",
            fontSize = 11.sp, color = Faint,
            modifier = Modifier.padding(16.dp)
        )
    }
}
