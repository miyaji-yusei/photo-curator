package app.photocurator.next

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

/**
 * 広い画面での置き方。
 *
 * この端末は開くと 2,448px ある。**文字と操作は横に伸ばさない。**
 * 全幅のボタンは押す場所が遠くなるし、1 行が長すぎて読みにくい。
 * 一方で**写真は伸ばす**。畳んだときも開いたときも、写真に使える面積を
 * 減らさないのがこのアプリの要点なので、選別画面はここを通さない。
 */
private val READING_WIDTH = 620.dp

/** 文字と操作のための帯。狭い画面ではそのまま、広い画面では中央に寄せる。 */
@Composable
fun Reading(
    modifier: Modifier = Modifier,
    verticalArrangement: Arrangement.Vertical = Arrangement.Top,
    content: @Composable ColumnScope.() -> Unit
) {
    Row(modifier.fillMaxWidth(), horizontalArrangement = Arrangement.Center) {
        Column(
            Modifier.widthIn(max = READING_WIDTH).fillMaxWidth(),
            verticalArrangement = verticalArrangement,
            horizontalAlignment = Alignment.Start,
            content = content
        )
    }
}
