package app.photocurator.next

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * 解析できなかったファイルの一覧（U58）。**名前と理由と、どう扱うか**。
 *
 * 非対応は「原本が変わるまで試さず、選別の対象にも入れない」、一時的は「次に開いたときにもう一度」。
 * 見るだけで、何も消さない・何も変えない。
 */
@Composable
fun FailuresDialog(rows: List<FailureRow>, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("解析できなかったファイル（${rows.size} 件）") },
        text = {
            LazyColumn(Modifier.fillMaxWidth().heightIn(max = 420.dp)) {
                for (kind in listOf(FailureKind.Unsupported, FailureKind.Transient)) {
                    val group = rows.filter { it.kind == kind }
                    if (group.isEmpty()) continue
                    item {
                        Text(
                            failureHeading(kind),
                            fontSize = 13.sp, fontWeight = FontWeight.SemiBold,
                            modifier = Modifier.padding(top = 8.dp, bottom = 2.dp)
                        )
                        Text(
                            failureNote(kind),
                            fontSize = 11.sp, color = Faint,
                            modifier = Modifier.padding(bottom = 4.dp)
                        )
                    }
                    items(group, key = { it.relativePath }) { row ->
                        Column(Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
                            Text(row.relativePath, fontSize = 13.sp, maxLines = 2, overflow = TextOverflow.Ellipsis)
                            Text(row.reason, fontSize = 11.sp, color = Faint)
                        }
                    }
                }
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text("閉じる") } }
    )
}

internal fun failureHeading(kind: FailureKind): String = when (kind) {
    FailureKind.Unsupported -> "対応していない形式"
    FailureKind.Transient -> "一時的に読めなかった"
}

internal fun failureNote(kind: FailureKind): String = when (kind) {
    FailureKind.Unsupported -> "原本が変わるまで試しません。選別の対象にも入れません。"
    FailureKind.Transient -> "次に開いたときにもう一度試します。"
}
