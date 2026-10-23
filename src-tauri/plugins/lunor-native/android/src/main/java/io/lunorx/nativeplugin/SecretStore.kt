package io.lunorx.nativeplugin

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Provider and session secrets. The AES-GCM key lives in the Android Keystore and never leaves it; only ciphertext is
 * written to app-private preferences. There is no plaintext fallback: any failure is reported to the caller.
 */
class SecretStore(context: Context) {
    private val prefs = context.getSharedPreferences("lunor_secrets", Context.MODE_PRIVATE)

    private fun key(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        generator.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return generator.generateKey()
    }

    private fun slot(service: String, account: String) = "$service/$account"

    fun get(service: String, account: String): String? {
        val raw = prefs.getString(slot(service, account), null) ?: return null
        val bytes = Base64.decode(raw, Base64.NO_WRAP)
        val cipher = Cipher.getInstance(TRANSFORM)
        cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, bytes.copyOfRange(0, IV)))
        return String(cipher.doFinal(bytes, IV, bytes.size - IV), Charsets.UTF_8)
    }

    fun set(service: String, account: String, value: String) {
        val cipher = Cipher.getInstance(TRANSFORM)
        cipher.init(Cipher.ENCRYPT_MODE, key())
        val sealed = cipher.iv + cipher.doFinal(value.toByteArray(Charsets.UTF_8))
        check(prefs.edit().putString(slot(service, account), Base64.encodeToString(sealed, Base64.NO_WRAP)).commit())
    }

    fun delete(service: String, account: String) {
        check(prefs.edit().remove(slot(service, account)).commit())
    }

    private companion object {
        const val ALIAS = "lunorx-secrets-v1"
        const val TRANSFORM = "AES/GCM/NoPadding"
        const val IV = 12
    }
}
