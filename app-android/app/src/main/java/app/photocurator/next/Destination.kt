@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package app.photocurator.next

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import coil.compose.AsyncImage
import coil.request.ImageRequest

/**
 * 移し先を選ぶ。
 *
 * **行き先はフォルダそのもの**（"Pictures/Camera/" のような置き場所）で、
 * 表示名ではない。同じ名前のフォルダが 2 つあることは実際にあるので、
 * 名前だけで決めると、どこへ移ったのか説明できなくなる。
 */
@Composable
fun DestinationSheet(
    from: Project,
    count: Int,
    onPick: (Album) -> Unit,
    onDismiss: () -> Unit
) {
    val context = LocalContext.current
    var albums by remember { mutableStateOf<List<Album>>(emptyList()) }
    var note by remember { mutableStateOf("読み込み中…") }

    LaunchedEffect(from.id) {
        // いま選別しているフォルダは出さない。**同じ所へ「移す」は何も起きない。**
        albums = Photos.albums(context)
            .filter { it.id != from.source.key && it.relativeDir.isNotBlank() }
        note = if (albums.isEmpty()) "ほかに移せるフォルダがありません" else ""
    }

    ModalBottomSheet(onDismissRequest = onDismiss, containerColor = Surface) {
        Column(Modifier.padding(horizontal = 16.dp).padding(bottom = 16.dp)) {
            Text("$count 枚の移し先", fontSize = 16.sp, fontWeight = FontWeight.SemiBold)
            Text(
                // **原本が動くことを、選ぶ前に言う。**
                "選ぶと、端末の中の置き場所が変わります。写真は消えません。",
                fontSize = 12.sp, color = Faint,
                modifier = Modifier.padding(top = 4.dp, bottom = 12.dp)
            )
            if (note.isNotEmpty()) {
                Text(note, fontSize = 13.sp, color = Faint, modifier = Modifier.padding(vertical = 24.dp))
            }
            LazyColumn(Modifier.heightIn(max = 420.dp)) {
                items(albums, key = { it.id }) { album ->
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .clip(RoundedCornerShape(10.dp))
                            .clickable { onPick(album) }
                            .padding(vertical = 8.dp, horizontal = 4.dp),
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        AsyncImage(
                            model = ImageRequest.Builder(LocalContext.current)
                                .data(
                                    android.content.ContentUris.withAppendedId(
                                        android.provider.MediaStore.Images.Media.EXTERNAL_CONTENT_URI,
                                        album.coverId
                                    )
                                ).size(160).build(),
                            contentDescription = null,
                            imageLoader = Images.loader(LocalContext.current),
                            contentScale = ContentScale.Crop,
                            modifier = Modifier.size(44.dp).clip(RoundedCornerShape(8.dp)).background(Tile)
                        )
                        Spacer(Modifier.width(12.dp))
                        Column(Modifier.weight(1f)) {
                            Text(album.name, fontSize = 15.sp)
                            // **どこへ移るのかを実際の道筋で見せる。**
                            Text(album.relativeDir, fontSize = 11.sp, color = Faint)
                        }
                        Icon(Icons.Filled.ChevronRight, null, Modifier.size(18.dp), tint = Faint)
                    }
                }
            }
        }
    }
}
