package com.cernauskas.shoppinglist.ui

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext

/**
 * Material 3, with the device's own colours where the device has them.
 *
 * Dynamic colour is the Android idiom and the reason not to port the iOS palette:
 * from Android 12 an app is expected to take its scheme from the wallpaper, so this
 * app looks like it belongs on the phone it is installed on rather than like a
 * visitor. The fixed scheme below is for older devices, and is a greengrocer's green
 * rather than the default purple.
 */
private val LightScheme = lightColorScheme(
    primary = Color(0xFF3B6939),
    onPrimary = Color.White,
    primaryContainer = Color(0xFFBCF0B4),
    onPrimaryContainer = Color(0xFF002105),
    secondary = Color(0xFF52634F),
    tertiary = Color(0xFF39656B),
)

private val DarkScheme = darkColorScheme(
    primary = Color(0xFFA1D399),
    onPrimary = Color(0xFF0A390F),
    primaryContainer = Color(0xFF235024),
    onPrimaryContainer = Color(0xFFBCF0B4),
    secondary = Color(0xFFB9CCB4),
    tertiary = Color(0xFFA1CED5),
)

@Composable
fun ShoppingTheme(content: @Composable () -> Unit) {
    val dark = isSystemInDarkTheme()
    val dynamic = Build.VERSION.SDK_INT >= Build.VERSION_CODES.S
    val context = LocalContext.current

    val scheme = when {
        dynamic && dark -> dynamicDarkColorScheme(context)
        dynamic -> dynamicLightColorScheme(context)
        dark -> DarkScheme
        else -> LightScheme
    }

    MaterialTheme(colorScheme = scheme, content = content)
}
