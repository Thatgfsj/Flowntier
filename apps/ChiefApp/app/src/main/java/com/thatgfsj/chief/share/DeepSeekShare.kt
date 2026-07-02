package com.thatgfsj.chief.share

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.widget.Toast
import com.thatgfsj.chief.DrawnCard

/**
 * v0.2.1 (event 000076): shares the tarot draw to DeepSeek
 * via `Intent.ACTION_SEND` with `setPackage("com.deepseek.chat")`.
 * Same pattern as IChingOracle's
 * `com.thatgfsj.iching.ui.oracle.DeepSeekShare` — ported to
 * the chief app's domain (tarot, not hexagram).
 *
 * The chairman's chosen share text (one format, applies to
 * both single-card and three-card-spread):
 *
 *     我抽取了塔罗牌，这是我抽到的卡牌：
 *
 *     <card body>
 *
 *     我要问的问题是：
 *
 * For a single card:
 *
 *     <牌名>（<name_en>）<顺位/逆位>
 *
 * For a three-card spread:
 *
 *     过去：<牌名>（<name_en>）<顺位/逆位>
 *     现在：<牌名>（<name_en>）<顺位/逆位>
 *     未来：<牌名>（<name_en>）<顺位/逆位>
 *
 * Orientation is the standard Rider-Waite term:
 * 顺位 (upright) / 逆位 (reversed). The first one is the
 * default; the second applies when the card was drawn
 * face-down. The IChingOracle's example used 卦象 names with
 * 卦辞 / 象传; the chief app's v0.2 doesn't yet carry poetic
 * meanings in cards.json (v0.3 will), so the share text is
 * intentionally short.
 */
object DeepSeekShare {
    private const val DEEPSEEK_PACKAGE = "com.deepseek.chat"
    private const val HEADER = "我抽取了塔罗牌，这是我抽到的卡牌："
    private const val PROMPT = "我要问的问题是："

    /**
     * Build the share text. One format, both modes. Single
     * card uses the format `<牌名>（<name_en>）<顺位/逆位>` on
     * a single line; three-card spread uses one line per
     * position with the labels 过去 / 现在 / 未来.
     */
    fun formatShareText(drawn: List<DrawnCard>): String = buildString {
        appendLine(HEADER)
        appendLine()
        if (drawn.size == 1) {
            appendCardLine(drawn.first(), positionLabel = null)
        } else {
            val positions = listOf("过去", "现在", "未来")
            drawn.forEachIndexed { i, c ->
                appendCardLine(c, positionLabel = positions.getOrNull(i))
            }
        }
        appendLine()
        append(PROMPT)
    }

    /**
     * One line per card: optional position prefix + the card
     * name + the orientation. Trailing newline (added by the
     * caller via appendLine). Reversed vs upright is the
     * chairman's chosen terminology.
     */
    private fun StringBuilder.appendCardLine(c: DrawnCard, positionLabel: String?) {
        val orientation = if (c.reversed) "逆位" else "顺位"
        val prefix = if (positionLabel != null) "$positionLabel：" else ""
        appendLine(
            "$prefix${c.card.name_zh}（${c.card.name_en}）$orientation"
        )
    }

    /**
     * Fire the chooser. Tries the package-scoped intent first
     * (Android 11+ requires the <queries> declaration in the
     * manifest for the package to be visible). Falls back to a
     * generic share chooser if DeepSeek is not installed. If no
     * app can handle text/plain at all, falls back to a Toast.
     */
    fun shareToDeepSeek(context: Context, drawn: List<DrawnCard>) {
        val text = formatShareText(drawn)
        val packageIntent = Intent(Intent.ACTION_SEND).apply {
            type = "text/plain"
            putExtra(Intent.EXTRA_TEXT, text)
            setPackage(DEEPSEEK_PACKAGE)
        }
        val finalIntent: Intent = try {
            if (context.packageManager.resolveActivity(
                    packageIntent, 0
                ) != null
            ) {
                Intent.createChooser(packageIntent, "分享到 DeepSeek")
            } else {
                Intent.createChooser(genericShareIntent(text), "分享卡牌")
            }
        } catch (_: Exception) {
            Intent.createChooser(genericShareIntent(text), "分享卡牌")
        }
        finalIntent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        try {
            context.startActivity(finalIntent)
        } catch (_: ActivityNotFoundException) {
            Toast.makeText(
                context, "未找到可分享的应用", Toast.LENGTH_SHORT
            ).show()
        }
    }

    private fun genericShareIntent(text: String): Intent =
        Intent(Intent.ACTION_SEND).apply {
            type = "text/plain"
            putExtra(Intent.EXTRA_TEXT, text)
        }
}
