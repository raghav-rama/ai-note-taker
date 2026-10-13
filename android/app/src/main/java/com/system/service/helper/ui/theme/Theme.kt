package com.system.service.helper.ui.theme

import android.app.Activity
import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.SideEffect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.core.view.WindowCompat

private val LightColorScheme = lightColorScheme(
    primary = Color(0xFF1A6B52),
    onPrimary = Color.White,
    primaryContainer = Color(0xFFA8F0D4),
    onPrimaryContainer = Color(0xFF002117),
    secondary = Color(0xFF4B635A),
    onSecondary = Color.White,
    secondaryContainer = Color(0xFFCDE8DC),
    onSecondaryContainer = Color(0xFF072018),
    tertiary = Color(0xFF406376),
    onTertiary = Color.White,
    tertiaryContainer = Color(0xFFC3E8FE),
    onTertiaryContainer = Color(0xFF001E2D),
    background = Color(0xFFFBFDF9),
    onBackground = Color(0xFF191C1B),
    surface = Color(0xFFFBFDF9),
    onSurface = Color(0xFF191C1B),
    error = Color(0xFFBA1A1A),
    onError = Color.White,
)

private val DarkColorScheme = darkColorScheme(
    primary = Color(0xFF8CD4B8),
    onPrimary = Color(0xFF00382A),
    primaryContainer = Color(0xFF00513D),
    onPrimaryContainer = Color(0xFFA8F0D4),
    secondary = Color(0xFFB2CCC1),
    onSecondary = Color(0xFF1D352C),
    secondaryContainer = Color(0xFF344C43),
    onSecondaryContainer = Color(0xFFCDE8DC),
    tertiary = Color(0xFFA7CCE1),
    onTertiary = Color(0xFF0B3446),
    tertiaryContainer = Color(0xFF274B5D),
    onTertiaryContainer = Color(0xFFC3E8FE),
    background = Color(0xFF191C1B),
    onBackground = Color(0xFFE1E3DF),
    surface = Color(0xFF191C1B),
    onSurface = Color(0xFFE1E3DF),
    error = Color(0xFFFFB4AB),
    onError = Color(0xFF690005),
)

@Composable
fun AppTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    dynamicColor: Boolean = true,
    content: @Composable () -> Unit,
) {
    val colorScheme = when {
        dynamicColor && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S -> {
            val context = LocalContext.current
            if (darkTheme) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
        }
        darkTheme -> DarkColorScheme
        else -> LightColorScheme
    }

    val view = LocalView.current
    if (!view.isInEditMode) {
        SideEffect {
            val window = (view.context as Activity).window
            window.statusBarColor = colorScheme.surface.toArgb()
            WindowCompat.getInsetsController(window, view).isAppearanceLightStatusBars = !darkTheme
        }
    }

    MaterialTheme(
        colorScheme = colorScheme,
        content = content,
    )
}
