package app.photocurator.next

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import android.util.Log
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.File
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * NAS のつなぎ先。**パスワードだけ扱いが違う。**
 *
 * ホスト・共有名・ユーザー名は普通に保存する。毎回入れ直すのは煩わしいだけで、
 * 漏れて困る度合いも低い。パスワードは**トグルが on のときだけ**、
 * 端末の Keystore で暗号化して保存する。off ならメモリにしか置かない。
 */
data class Nas(
    val id: String,
    /** 人が付けた名前。「home-nas」など。**画面にはこれを出す。** */
    val label: String,
    val host: String,
    val share: String,
    val user: String,
    /** パスワードを端末に保存するか。 */
    val remember: Boolean
)

/** つながっているか。**接続中／未接続／失敗**の 3 つだけ。 */
enum class NasState { Connected, Disconnected, Failed }

object NasStore {
    private const val TAG = "NasStore"
    private const val FILE = "nas.json"
    private const val KEY_ALIAS = "photo-curator-nas"

    private fun file(context: Context) = File(context.filesDir, FILE)

    suspend fun all(context: Context): List<Nas> = withContext(Dispatchers.IO) {
        val target = file(context)
        if (!target.exists()) return@withContext emptyList()
        try {
            val array = org.json.JSONArray(target.readText())
            (0 until array.length()).map { at ->
                val entry = array.getJSONObject(at)
                Nas(
                    id = entry.getString("id"),
                    label = entry.getString("label"),
                    host = entry.getString("host"),
                    share = entry.getString("share"),
                    user = entry.getString("user"),
                    remember = entry.optBoolean("remember", false)
                )
            }
        } catch (error: Exception) {
            Log.w(TAG, "NAS の設定を読めなかった", error)
            emptyList()
        }
    }

    suspend fun save(context: Context, list: List<Nas>) = withContext(Dispatchers.IO) {
        try {
            val array = org.json.JSONArray()
            for (nas in list) {
                array.put(
                    org.json.JSONObject()
                        .put("id", nas.id)
                        .put("label", nas.label)
                        .put("host", nas.host)
                        .put("share", nas.share)
                        .put("user", nas.user)
                        .put("remember", nas.remember)
                )
            }
            val target = file(context)
            val temporary = File(target.parentFile, "${target.name}.writing")
            temporary.writeText(array.toString())
            if (!temporary.renameTo(target)) {
                temporary.copyTo(target, overwrite = true)
                temporary.delete()
            }
            Unit
        } catch (error: Exception) {
            Log.w(TAG, "NAS の設定を保存できなかった", error)
        }
    }

    suspend fun upsert(context: Context, nas: Nas) {
        val existing = all(context)
        save(
            context,
            if (existing.any { it.id == nas.id }) existing.map { if (it.id == nas.id) nas else it }
            else existing + nas
        )
    }

    suspend fun remove(context: Context, id: String) {
        save(context, all(context).filterNot { it.id == id })
        forgetPassword(context, id)
    }

    // ---------------------------------------------------------------------
    // パスワード
    // ---------------------------------------------------------------------

    /**
     * 端末の鍵。**アプリの中にも書かない。** Keystore の中で作られ、
     * 取り出せない。アプリを消せば鍵も消えるので、保存した文字列も読めなくなる。
     */
    private fun secretKey(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getEntry(KEY_ALIAS, null) as? KeyStore.SecretKeyEntry)?.let { return it.secretKey }
        val generator = KeyGenerator.getInstance(
            KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore"
        )
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

    private fun passwordFile(context: Context, id: String) =
        File(context.filesDir, "nas-secret-$id.bin")

    /** 保存する。**呼ばれるのはトグルが on のときだけ。** */
    suspend fun rememberPassword(context: Context, id: String, password: String) =
        withContext(Dispatchers.IO) {
            try {
                val cipher = Cipher.getInstance("AES/GCM/NoPadding")
                cipher.init(Cipher.ENCRYPT_MODE, secretKey())
                val sealed = cipher.doFinal(password.toByteArray(Charsets.UTF_8))
                // 初期化ベクトルは暗号文と一緒に置く。秘密ではない。
                val payload = org.json.JSONObject()
                    .put("iv", Base64.encodeToString(cipher.iv, Base64.NO_WRAP))
                    .put("data", Base64.encodeToString(sealed, Base64.NO_WRAP))
                passwordFile(context, id).writeText(payload.toString())
                Unit
            } catch (error: Exception) {
                Log.w(TAG, "パスワードを保存できなかった", error)
            }
        }

    suspend fun password(context: Context, id: String): String? = withContext(Dispatchers.IO) {
        val target = passwordFile(context, id)
        if (!target.exists()) return@withContext null
        try {
            val payload = org.json.JSONObject(target.readText())
            val iv = Base64.decode(payload.getString("iv"), Base64.NO_WRAP)
            val data = Base64.decode(payload.getString("data"), Base64.NO_WRAP)
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.DECRYPT_MODE, secretKey(), GCMParameterSpec(128, iv))
            String(cipher.doFinal(data), Charsets.UTF_8)
        } catch (error: Exception) {
            // 鍵が変わった・壊れた。**読めないものは無かったことにする。**
            Log.w(TAG, "パスワードを読めなかった", error)
            null
        }
    }

    suspend fun forgetPassword(context: Context, id: String) = withContext(Dispatchers.IO) {
        passwordFile(context, id).delete()
        Unit
    }
}
