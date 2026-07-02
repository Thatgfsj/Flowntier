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

// v0.2.0 (event 000075): the chief app's palette now matches
// the chief website (workspace/tarot/index.html — the 78-card
// 67 KB site the chief wrote). The website uses deep purple-
// black background + gold + champagne text + Cormorant
// Garamond serif. The chief wanted the Android app to look
// "跟网站一样" — visual parity with the web site, not
// with the desktop Flowntier product or with iching-oracle.
//
// Why we change from the previous teal palette: the chairman's
// direct quote for this event was "塔罗牌图片你自己去这里
// 找：https://www.shenpowang.com/taluopai/jieshi/" — and the
// existing teal/iching-oracle look is too "office product"
// for a tarot divination app. Tarot wants mystical gold.
// The desktop Flowntier product stays teal (it isn't a
// divination tool); only the chief's Android tarot app
// adopts the gold/purple palette.
private val DarkColorScheme = darkColorScheme(
    primary = Color(0xFFD4AF37),         // gold
    onPrimary = Color(0xFF1A0F2E),
    secondary = Color(0xFFF5E7A0),       // champagne gold
    onSecondary = Color(0xFF1A0F2E),
    tertiary = Color(0xFF6B3FA0),        // purple
    background = Color(0xFF040214),      // deep purple-black
    onBackground = Color(0xFFF5E7C8),    // champagne text
    surface = Color(0xFF0D052E),
    onSurface = Color(0xFFF5E7C8),
    surfaceVariant = Color(0xFF1A0F3E),
    onSurfaceVariant = Color(0xFFA89CC4),  // muted lavender
    error = Color(0xFFE85A3C),
)

private val LightColorScheme = lightColorScheme(
    primary = Color(0xFF6B3FA0),
    onPrimary = Color(0xFFFFFFFF),
    secondary = Color(0xFFD4AF37),
    onSecondary = Color(0xFF1A0F2E),
    tertiary = Color(0xFF8A4FB8),
    background = Color(0xFFFFF8E7),
    onBackground = Color(0xFF1A0F2E),
    surface = Color(0xFFFFFCF0),
    onSurface = Color(0xFF1A0F2E),
    surfaceVariant = Color(0xFFEFE0C0),
    onSurfaceVariant = Color(0xFF6B5A2E),
    error = Color(0xFFB83A1C),
)

@Composable
fun ChiefAppTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    dynamicColor: Boolean = false,
    content: @Composable () -> Unit,
) {
    // The chief website is always dark (purple-black bg).
    // Force dark theme regardless of system setting so the
    // visual identity is consistent across users. The
    // lightColorScheme is kept around for a future "light
    // mode" toggle if the chairman asks.
    val colorScheme = when {
        dynamicColor && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S -> {
            val context = LocalContext.current
            if (darkTheme) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
        }
        else -> DarkColorScheme
    }

    val view = LocalView.current
    if (!view.isInEditMode) {
        SideEffect {
            val window = (view.context as Activity).window
            window.statusBarColor = colorScheme.background.toArgb()
            WindowCompat.getInsetsController(window, view).isAppearanceLightStatusBars = false
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
