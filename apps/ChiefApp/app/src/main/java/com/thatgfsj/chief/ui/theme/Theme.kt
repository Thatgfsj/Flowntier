package com.thatgfsj.chief.ui.theme

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
import androidx.compose.ui.unit.sp
import androidx.core.view.WindowCompat

// v0.1.0 (event 000070): ink-and-paper palette shared with
// IChingOracle (com.thatgfsj.iching) — calm, contemplative
// surface for a divination-adjacent workflow app. Chief
// 调度员 + 塔罗牌任务用同一套主题,确保 Flowntier 全家桶
// 视觉一致。
private val DarkColorScheme = darkColorScheme(
    primary = Color(0xFF80CBC4),
    onPrimary = Color(0xFF003731),
    secondary = Color(0xFFB0BEC5),
    onSecondary = Color(0xFF1A2C2E),
    tertiary = Color(0xFFBCAAA4),
    background = Color(0xFF121417),
    onBackground = Color(0xFFE8E6E1),
    surface = Color(0xFF1A1D21),
    onSurface = Color(0xFFE8E6E1),
    surfaceVariant = Color(0xFF252A30),
    onSurfaceVariant = Color(0xFFC0BDB6),
    error = Color(0xFFE57373),
)

private val LightColorScheme = lightColorScheme(
    primary = Color(0xFF00897B),
    onPrimary = Color(0xFFFFFFFF),
    secondary = Color(0xFF607D8B),
    onSecondary = Color(0xFFFFFFFF),
    tertiary = Color(0xFF8D6E63),
    background = Color(0xFFF5F1E8),
    onBackground = Color(0xFF1A1A1A),
    surface = Color(0xFFFFFDF7),
    onSurface = Color(0xFF1A1A1A),
    surfaceVariant = Color(0xFFEBE5D6),
    onSurfaceVariant = Color(0xFF555555),
    error = Color(0xFFB71C1C),
)

@Composable
fun ChiefAppTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    dynamicColor: Boolean = false,
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
            window.statusBarColor = colorScheme.background.toArgb()
            WindowCompat.getInsetsController(window, view).isAppearanceLightStatusBars = !darkTheme
        }
    }

    MaterialTheme(
        colorScheme = colorScheme,
        typography = Typography,
        content = content,
    )
}

private val Typography = androidx.compose.material3.Typography(
    displayLarge = androidx.compose.ui.text.TextStyle(
        fontFamily = androidx.compose.ui.text.font.FontFamily.Serif,
        fontWeight = androidx.compose.ui.text.font.FontWeight.Normal,
    ),
    bodyLarge = androidx.compose.ui.text.TextStyle(
        fontFamily = androidx.compose.ui.text.font.FontFamily.Serif,
        fontWeight = androidx.compose.ui.text.font.FontWeight.Normal,
    ),
    bodyMedium = androidx.compose.ui.text.TextStyle(
        fontFamily = androidx.compose.ui.text.font.FontFamily.SansSerif,
    ),
    labelSmall = androidx.compose.ui.text.TextStyle(
        fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
        letterSpacing = 0.1.sp,
    ),
)
