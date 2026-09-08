package app.photocurator.next

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * 取り消せない操作の確認。**出すのはここだけ。**
 *
 * 中断・1 つ戻す・ラウンド完了には出さない。毎回確認を挟むと、本当に
 * 確かめてほしいときにも読まずに押されるようになる。
 *
 * **原本を書き換える操作は色で分ける。** このアプリの約束は「原本には
 * 触らない」なので、その約束の外に出る 4 つ（お気に入り・アルバムへ移動・
 * NAS で移動・メタデータ書き込み）だけは、赤い 1 行とボタンの色で
 * 「いつもと違うこと」を先に見せる。
 */
@Composable
fun ConfirmDialog(
    title: String,
    body: String,
    confirmLabel: String,
    /** 原本を書き換えるか。true なら赤字の 1 行と error 地のボタン。 */
    touchesOriginals: Boolean = false,
    onConfirm: () -> Unit,
    onDismiss: () -> Unit
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = {
            Column {
                if (touchesOriginals) {
                    Text(
                        "原本を書き換えます",
                        color = Warn, fontWeight = FontWeight.Bold, fontSize = 14.sp,
                        modifier = Modifier.padding(bottom = 6.dp)
                    )
                }
                Text(body)
            }
        },
        confirmButton = {
            if (touchesOriginals) {
                Button(
                    onClick = onConfirm,
                    colors = ButtonDefaults.buttonColors(
                        containerColor = Warn, contentColor = Color.Black
                    )
                ) { Text(confirmLabel, fontWeight = FontWeight.Bold) }
            } else {
                TextButton(onClick = onConfirm) { Text(confirmLabel) }
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("やめる") } }
    )
}
