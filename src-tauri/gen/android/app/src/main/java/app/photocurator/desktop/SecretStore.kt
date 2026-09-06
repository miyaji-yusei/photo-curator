package app.photocurator.desktop

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import android.util.Log
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * NAS のパスワードを端末に預かる口。**利用者が「保存する」を選んだときだけ使う。**
 *
 * 鍵は Android Keystore が持ち、**このアプリの外へは出ない**（端末から鍵材料を
 * 取り出せない）。暗号文だけを普通の SharedPreferences に置く。
 *
 * ここを SharedPreferences への平文にしないのは、端末が root 化されていたり
 * バックアップが取られたときに、NAS の資格情報がそのまま読めてしまうため。
 *
 * `androidx.security:security-crypto` を使わないのは、依存を増やさずに
 * 同じことができるから。あちらも中身は Keystore の鍵で包むだけ。
 */
object SecretStore {
    private const val TAG = "SecretStore"
    private const val PREFS = "nas-secret"
    private const val KEY_ALIAS = "nas-password"
    private const val VALUE = "password"

    /** GCM の推奨。**IV は毎回作り、暗号文の前に付けて保存する。** */
    private const val IV_BYTES = 12
    private const val TAG_BITS = 128

    @Volatile
    private var context: Context? = null

    /** `MainActivity` から一度だけ渡してもらう。 */
    @JvmStatic
    fun attach(context: Context) {
        this.context = context.applicationContext
    }

    private fun prefs() = context?.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    /**
     * この用途の鍵。無ければ作る。
     *
     * 画面ロックを要求しない（`setUserAuthenticationRequired` を立てない）のは、
     * 選別中に何度も読むため。守りたいのは「端末の外へ持ち出されること」で、
     * 端末を開ける人からの防御ではない。
     */
    private fun key(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getEntry(KEY_ALIAS, null) as? KeyStore.SecretKeyEntry)?.let { return it.secretKey }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        generator.init(
            KeyGenParameterSpec.Builder(
                KEY_ALIAS,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .build()
        )
        return generator.generateKey()
    }

    /** 空文字を渡すと消す。 */
    @JvmStatic
    @Synchronized
    fun savePassword(password: String): String {
        val prefs = prefs() ?: return "端末の保存領域を使えません。"
        if (password.isEmpty()) {
            prefs.edit().remove(VALUE).apply()
            return ""
        }
        return try {
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.ENCRYPT_MODE, key())
            val encrypted = cipher.doFinal(password.toByteArray(Charsets.UTF_8))
            val packed = cipher.iv + encrypted
            prefs.edit().putString(VALUE, Base64.encodeToString(packed, Base64.NO_WRAP)).apply()
            ""
        } catch (error: Exception) {
            Log.w(TAG, "パスワードを保存できなかった。", error)
            "パスワードを保存できませんでした。"
        }
    }

    /** 保存が無ければ空文字。 */
    @JvmStatic
    @Synchronized
    fun loadPassword(): String {
        val stored = prefs()?.getString(VALUE, null) ?: return ""
        return try {
            val packed = Base64.decode(stored, Base64.NO_WRAP)
            if (packed.size <= IV_BYTES) return ""
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(
                Cipher.DECRYPT_MODE,
                key(),
                GCMParameterSpec(TAG_BITS, packed, 0, IV_BYTES)
            )
            String(
                cipher.doFinal(packed, IV_BYTES, packed.size - IV_BYTES),
                Charsets.UTF_8
            )
        } catch (error: Exception) {
            // 鍵が作り直された（アプリのデータ削除、端末の復元）。読めないだけで、
            // 入れ直してもらえば済む。**残骸は消しておく。**
            Log.w(TAG, "パスワードを読めなかった。消す。", error)
            prefs()?.edit()?.remove(VALUE)?.apply()
            ""
        }
    }
}
