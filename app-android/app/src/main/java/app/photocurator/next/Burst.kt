@file:OptIn(
    androidx.compose.material3.ExperimentalMaterial3Api::class,
    androidx.compose.foundation.ExperimentalFoundationApi::class
)

package app.photocurator.next

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
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

/**
 * 連写のまとまりを開く。**畳んだ中身を見せる場所。**
 *
 * 代表は既定で撮影順の先頭になるが、**先頭がぶれていることは普通にある。**
 * 代表 1 枚しか見えないと、その 1 枚の出来でまとまり全体の運命が決まり、
 * 畳んだことがそのまま取りこぼしになる。ここで選び直せるようにする。
 *
 * ここで星は動かない。選ぶのは「どれを見せるか」だけで、
 * 「どれを残すか」は今までどおり選別画面で決める。
 */
@Composable
fun BurstSheet(
    members: List<String>,
    shown: String,
    byPath: Map<String, Photo>,
    onPick: (String) -> Unit,
    onZoom: (Photo) -> Unit,
    onSplit: () -> Unit,
    onDismiss: () -> Unit
) {
    ModalBottomSheet(onDismissRequest = onDismiss, containerColor = Surface) {
        Column(Modifier.padding(horizontal = 16.dp).padding(bottom = 24.dp)) {
            Text(
                "連写 ${members.size} 枚",
                fontSize = 16.sp, fontWeight = FontWeight.SemiBold
            )
            Text(
                // **押すと何が起きるかを先に言う。** 星が動くのかどうかは、
                // ここで一番知りたいこと。
                "画面に出す 1 枚を選べます。星は動きません。" +
                    "残したときは、この ${members.size} 枚ぜんぶに同じ星が付きます。",
                fontSize = 12.sp, color = Faint,
                modifier = Modifier.padding(top = 4.dp, bottom = 12.dp)
            )

            LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                items(members, key = { it }) { path ->
                    val photo = byPath[path]
                    val here = path == shown
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Box(
                            Modifier
                                // シートの既定の高さに収まる大きさ。**選ぶための札**
                                // なので、確かめたいときは長押しで拡大に回す。
                                .height(150.dp)
                                .width(115.dp)
                                .clip(RoundedCornerShape(8.dp))
                                .background(Tile)
                                .then(
                                    if (here) Modifier.border(3.dp, Lime, RoundedCornerShape(8.dp))
                                    else Modifier
                                )
                                .combinedClickable(
                                    enabled = photo != null,
                                    onClick = { onPick(path) },
                                    // 選ぶ前に確かめたいので、ここでも長押しで拡大。
                                    onLongClick = { photo?.let(onZoom) }
                                )
                        ) {
                            if (photo == null) {
                                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                                    Text("読めません", fontSize = 11.sp, color = Faint)
                                }
                            } else {
                                AsyncImage(
                                    model = ImageRequest.Builder(LocalContext.current)
                                        .data(photo.uri).size(720).build(),
                                    contentDescription = photo.name,
                                    contentScale = ContentScale.Fit,
                                    modifier = Modifier.fillMaxSize()
                                )
                            }
                        }
                        Text(
                            if (here) "表示中" else "これにする",
                            fontSize = 11.sp,
                            color = if (here) Lime else Faint,
                            modifier = Modifier.padding(top = 6.dp)
                        )
                    }
                }
            }

            Row(
                Modifier.fillMaxWidth().padding(top = 12.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text("長押しで大きく見られます", fontSize = 11.sp, color = Faint)
                Spacer(Modifier.weight(1f))
                // まとめ方が違っていたとき用。**その場で組み直る。**
                TextButton(onClick = onSplit) {
                    Text("まとまりを解く", fontSize = 13.sp)
                }
            }
        }
    }
}
