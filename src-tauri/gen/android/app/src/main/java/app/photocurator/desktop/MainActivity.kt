package app.photocurator.desktop

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat

class MainActivity : TauriActivity() {
  /**
   * 写真を読む権限。**結果は待たない。**
   *
   * Rust 側は権限が無ければ空の一覧を受け取るだけで、画面はそれを
   * 「写真が見つかりません」として出す。ここで待ち合わせを作ると、
   * Activity の結果を Rust まで運ぶ仕掛けが要る。
   */
  private val requestPhotos =
    registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { }

  /**
   * フォルダ選択（SAF）。**NAS のベンダー製アプリが DocumentsProvider として
   * 登録されていれば、ここに NAS が現れる。**
   *
   * 結果は `TreeAccess` に置くだけ。Rust は「開く」→「あとで取りに来る」の
   * 2 段で読み、Activity Result を Rust まで運ぶ仕掛けを避ける。
   */
  private val pickTree =
    registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
      val uri = result.data?.data ?: return@registerForActivityResult
      // 権限を永続化する。**これが無いと再起動後に読めなくなる。**
      contentResolver.takePersistableUriPermission(
        uri,
        Intent.FLAG_GRANT_READ_URI_PERMISSION
      )
      TreeAccess.setPickedTree(uri.toString())
    }

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    PhotoAccess.attach(this)
    TreeAccess.attach(this)
    instance = this
    requestPhotoPermissions()
  }

  override fun onDestroy() {
    if (instance === this) instance = null
    super.onDestroy()
  }

  private fun requestPhotoPermissions() {
    val wanted = when {
      // Android 14 以降は「選んだ写真だけ」の部分許可がある。
      // 全件を求めて断られるより、部分許可でも受け取れるようにしておく。
      Build.VERSION.SDK_INT >= 34 -> arrayOf(
        Manifest.permission.READ_MEDIA_IMAGES,
        "android.permission.READ_MEDIA_VISUAL_USER_SELECTED"
      )
      Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU ->
        arrayOf(Manifest.permission.READ_MEDIA_IMAGES)
      else -> arrayOf(Manifest.permission.READ_EXTERNAL_STORAGE)
    }
    val missing = wanted.filter {
      ContextCompat.checkSelfPermission(this, it) != PackageManager.PERMISSION_GRANTED
    }
    if (missing.isNotEmpty()) requestPhotos.launch(missing.toTypedArray())
  }

  companion object {
    /** Rust からフォルダ選択を開くための入口。 */
    @Volatile
    private var instance: MainActivity? = null

    @JvmStatic
    fun openTreePicker() {
      val activity = instance ?: return
      activity.runOnUiThread {
        runCatching { activity.pickTree.launch(TreeAccess.pickIntent()) }
      }
    }
  }
}
