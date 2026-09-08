@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package app.photocurator.next

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Visibility
import androidx.compose.material.icons.filled.VisibilityOff
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.launch

/**
 * NAS のつなぎ先を編集する。
 *
 * **パスワードは、保存するかどうかを人が決める。** 既定は保存しない。
 * on にしたときだけ端末の Keystore で暗号化して置く。
 *
 * 「接続を確認」は保存する前に押せる。**入れた値が正しいかを、
 * 保存してから知るのでは遅い。**
 */
@Composable
fun NasEditSheet(
    existing: Nas?,
    onSaved: (Nas) -> Unit,
    onRemoved: (String) -> Unit,
    onDismiss: () -> Unit
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    var label by remember { mutableStateOf(existing?.label ?: "") }
    var host by remember { mutableStateOf(existing?.host ?: "") }
    var share by remember { mutableStateOf(existing?.share ?: "") }
    var user by remember { mutableStateOf(existing?.user ?: "") }
    var password by remember { mutableStateOf("") }
    var shown by remember { mutableStateOf(false) }
    var remember_ by remember { mutableStateOf(existing?.remember ?: false) }
    var checking by remember { mutableStateOf(false) }
    var result by remember { mutableStateOf<String?>(null) }
    var ok by remember { mutableStateOf(false) }

    // 保存ずみのパスワードがあれば入れておく。**あることは伏せない。**
    LaunchedEffect(existing?.id) {
        val id = existing?.id ?: return@LaunchedEffect
        NasStore.password(context, id)?.let { password = it }
    }

    fun current() = Nas(
        id = existing?.id ?: "nas${System.currentTimeMillis()}",
        label = label.trim().ifEmpty { host.trim() },
        host = host.trim(),
        share = share.trim(),
        user = user.trim(),
        remember = remember_
    )

    val ready = host.isNotBlank() && share.isNotBlank() && user.isNotBlank()

    val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheet,
        containerColor = Surface,
        // **下の帯まで自分の色で塗る。** 既定だとナビゲーションバーのところが
        // 白く残り、一番下のボタンに被る。
        contentWindowInsets = { WindowInsets(0) }
    ) {
        Column(Modifier.padding(horizontal = 20.dp).padding(bottom = 20.dp)) {
            Text(
                if (existing == null) "NAS を追加" else "NAS の設定",
                fontSize = 18.sp, fontWeight = FontWeight.SemiBold
            )
            Spacer(Modifier.height(14.dp))

            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Column(Modifier.weight(1f)) {
                    Field("名前", label, { label = it }, "home-nas")
                    Field("ホスト名または IP", host, { host = it }, "192.168.11.8")
                    Field("共有名", share, { share = it }, "Share")
                }
                Column(Modifier.weight(1f)) {
                    Field("ユーザー名", user, { user = it }, "")
                    OutlinedTextField(
                        value = password,
                        onValueChange = { password = it },
                        singleLine = true,
                        label = { Text("パスワード") },
                        visualTransformation =
                            if (shown) VisualTransformation.None else PasswordVisualTransformation(),
                        trailingIcon = {
                            // **右端に目のアイコン。** 打ち間違いは、見えないと直せない。
                            IconButton(onClick = { shown = !shown }) {
                                Icon(
                                    if (shown) Icons.Filled.VisibilityOff else Icons.Filled.Visibility,
                                    if (shown) "隠す" else "表示する"
                                )
                            }
                        },
                        modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp)
                    )
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(
                            "パスワードをこの端末に保存する",
                            fontSize = 12.sp, modifier = Modifier.weight(1f)
                        )
                        Switch(
                            checked = remember_,
                            onCheckedChange = { remember_ = it },
                            colors = SwitchDefaults.colors(
                                checkedThumbColor = Color.Black, checkedTrackColor = Lime
                            )
                        )
                    }
                    Text(
                        if (remember_) "端末の鍵で暗号化して保存します"
                        else "off のあいだは、このアプリを閉じると忘れます",
                        fontSize = 11.sp, color = Faint
                    )
                }
            }

            result?.let {
                Spacer(Modifier.height(10.dp))
                Text(it, fontSize = 12.sp, color = if (ok) Lime else Color(0xFFFF8A80))
            }

            Spacer(Modifier.height(16.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (existing != null) {
                    TextButton(onClick = {
                        scope.launch {
                            NasStore.remove(context, existing.id)
                            onRemoved(existing.id)
                        }
                    }) { Text("削除", color = Color(0xFFFF8A80)) }
                }
                Spacer(Modifier.weight(1f))
                OutlinedButton(
                    onClick = {
                        checking = true
                        result = "つないでいます…"
                        ok = false
                        scope.launch {
                            when (val answer = Smb.check(current(), password)) {
                                is SmbResult.Ok -> {
                                    ok = true
                                    result = "つながりました（${answer.value} 件のフォルダ）"
                                }
                                is SmbResult.Failed -> {
                                    ok = false
                                    result = answer.reason
                                }
                            }
                            checking = false
                        }
                    },
                    enabled = ready && !checking,
                    shape = RoundedCornerShape(50)
                ) { Text("接続を確認") }
                Spacer(Modifier.width(8.dp))
                Button(
                    onClick = {
                        val nas = current()
                        scope.launch {
                            NasStore.upsert(context, nas)
                            // **トグルが off なら、前に保存した分も消す。**
                            // 「off にしたのに残っている」が一番まずい。
                            if (nas.remember) NasStore.rememberPassword(context, nas.id, password)
                            else NasStore.forgetPassword(context, nas.id)
                            Session.hold(nas.id, password)
                            onSaved(nas)
                        }
                    },
                    enabled = ready,
                    shape = RoundedCornerShape(50)
                ) { Text("保存", fontWeight = FontWeight.Bold) }
            }
        }
    }
}

@Composable
private fun Field(
    label: String,
    value: String,
    onChange: (String) -> Unit,
    hint: String
) {
    OutlinedTextField(
        value = value,
        onValueChange = onChange,
        singleLine = true,
        label = { Text(label) },
        placeholder = if (hint.isEmpty()) null else ({ Text(hint, color = Faint) }),
        modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp)
    )
}

/**
 * このアプリが動いているあいだだけ覚えておくパスワード。
 *
 * **保存トグルが off の人のための場所。** 一度入れれば、
 * アプリを閉じるまでは繰り返し聞かれない。閉じれば消える。
 */
object Session {
    private val held = HashMap<String, String>()

    fun hold(id: String, password: String) {
        held[id] = password
    }

    fun forget(id: String) {
        held.remove(id)
    }

    /** 保存ずみ → メモリ の順で探す。無ければ null（呼ぶ側が聞く）。 */
    suspend fun password(context: android.content.Context, nas: Nas): String? =
        if (nas.remember) NasStore.password(context, nas.id) ?: held[nas.id]
        else held[nas.id]
}
