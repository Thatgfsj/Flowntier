package com.thatgfsj.chief.ui.oracle

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.viewmodel.compose.viewModel
import com.thatgfsj.chief.TarotViewModel
import com.thatgfsj.chief.TarotUiState
import com.thatgfsj.chief.tarot.DrawnCard
import com.thatgfsj.chief.tarot.TarotListCard
import kotlinx.coroutines.delay
import kotlin.random.Random

/**
 * v0.1.0 (event 000074): single-screen tarot oracle. Mirrors
 * iching-oracle's IChingOracleScreen layout (centered content,
 * serif headings, ink-and-paper background) but renders TAROT
 * cards (78-card Rider-Waite) instead of 64 hexagrams, and
 * pulls them from the Flwntier runtime over JSON-RPC.
 *
 * Visual sequence:
 *   1. Home page: 78-card name cloud drifts in the
 *      background, centered title "塔罗", tagline "心诚则灵",
 *      one main button "点击抽取" + secondary "三卡阵 过去/现在/未来".
 *   2. Draw: 1.5s flip animation; each card rotates 0° → 360°
 *      with a brief scale-in.
 *   3. Loaded: card(s) shown face-up with name_zh, pinyin, en,
 *      upright/reversed meaning, and a "再抽一签" / "三卡阵"
 *      button row.
 */
@Composable
fun TarotScreen(viewModel: TarotViewModel = viewModel()) {
    val state by viewModel.state.collectAsState()
    val host by viewModel.host.collectAsState()
    val connected by viewModel.connected.collectAsState()

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background),
    ) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState()),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Header()
            Spacer(Modifier.height(8.dp))
            HostLine(host = host, connected = connected, onPing = viewModel::ping, onChange = viewModel::setHost)
            Spacer(Modifier.height(16.dp))

            when (val s = state) {
                is TarotUiState.Initial -> HomePage(
                    onDrawOne = viewModel::drawOne,
                    onDrawThree = viewModel::drawThree,
                )
                is TarotUiState.Drawing -> DrawingView(cards = s.cards)
                is TarotUiState.Loaded -> LoadedView(
                    cards = s.cards,
                    onDrawOne = viewModel::drawOne,
                    onDrawThree = viewModel::drawThree,
                    onClear = viewModel::clear,
                )
                is TarotUiState.Error -> ErrorView(s.message, viewModel::drawOne)
            }
        }
    }
}

// ── Header / home page ─────────────────────────────────

@Composable
private fun Header() {
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        Text(
            text = "塔罗",
            fontFamily = FontFamily.Serif,
            fontWeight = FontWeight.SemiBold,
            fontSize = 36.sp,
            color = MaterialTheme.colorScheme.primary,
        )
        Spacer(Modifier.height(2.dp))
        Text(
            text = "Flowntier · 心诚则灵",
            fontFamily = FontFamily.Serif,
            fontSize = 12.sp,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun HostLine(
    host: String,
    connected: Boolean?,
    onPing: () -> Unit,
    onChange: (String) -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        androidx.compose.material3.OutlinedTextField(
            value = host,
            onValueChange = onChange,
            label = { Text("runtime", fontSize = 10.sp) },
            singleLine = true,
            modifier = Modifier.weight(1f),
            textStyle = MaterialTheme.typography.bodySmall.copy(
                fontFamily = FontFamily.Monospace,
            ),
        )
        androidx.compose.material3.TextButton(
            onClick = onPing,
            contentPadding = PaddingValues(horizontal = 8.dp, vertical = 4.dp),
        ) {
            val color = when (connected) {
                null -> MaterialTheme.colorScheme.onSurfaceVariant
                true -> Color(0xFF2E7D32)
                false -> MaterialTheme.colorScheme.error
            }
            val label = when (connected) {
                null -> "ping"
                true -> "online"
                false -> "offline"
            }
            Text(
                text = label,
                fontSize = 10.sp,
                color = color,
            )
        }
    }
}

@Composable
private fun HomePage(
    onDrawOne: () -> Unit,
    onDrawThree: () -> Unit,
) {
    Box(modifier = Modifier.fillMaxSize()) {
        // 78-card name cloud drifts in the background — the
        // chairman's visual nod to iching-oracle's 64-gua word
        // cloud but using the full tarot deck.
        TarotNameCloud()
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(horizontal = 32.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center,
        ) {
            Text(
                text = "请在心中默念你的问题",
                fontFamily = FontFamily.Serif,
                fontSize = 11.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f),
            )
            Spacer(Modifier.height(56.dp))
            Button(
                onClick = onDrawOne,
                shape = RoundedCornerShape(28.dp),
                colors = ButtonDefaults.buttonColors(
                    containerColor = MaterialTheme.colorScheme.primary,
                ),
                contentPadding = PaddingValues(horizontal = 40.dp, vertical = 16.dp),
            ) {
                Text(
                    text = "点击抽取",
                    fontFamily = FontFamily.Serif,
                    fontSize = 18.sp,
                    color = MaterialTheme.colorScheme.onPrimary,
                )
            }
            Spacer(Modifier.height(20.dp))
            androidx.compose.material3.OutlinedButton(
                onClick = onDrawThree,
                shape = RoundedCornerShape(28.dp),
                contentPadding = PaddingValues(horizontal = 32.dp, vertical = 12.dp),
            ) {
                Text(
                    text = "三卡阵  ·  过去 / 现在 / 未来",
                    fontFamily = FontFamily.Serif,
                    fontSize = 14.sp,
                )
            }
        }
        Column(
            modifier = Modifier
                .align(Alignment.BottomCenter)
                .fillMaxWidth()
                .padding(bottom = 16.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text(
                text = "开发者:Thatgfsj",
                fontFamily = FontFamily.SansSerif,
                fontSize = 10.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.55f),
            )
            Text(
                text = "仓库:flowntier  ·  apps/ChiefApp",
                fontFamily = FontFamily.Monospace,
                fontSize = 10.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.55f),
            )
        }
    }
}

/**
 * 78-card name cloud — the tarot equivalent of iching-oracle's
 * 64-gua word cloud. We pull the names from a static subset
 * (built into the app for offline display) and scatter them
 * across the screen with a slow infinite vertical drift.
 */
@Composable
private fun TarotNameCloud() {
    val names = remember { TAROT_CLOUD_NAMES.shuffled(Random(42)) }
    val configuration = LocalConfiguration.current
    val density = LocalDensity.current
    val widthPx = with(density) { configuration.screenWidthDp.dp.toPx() }
    val heightPx = with(density) { configuration.screenHeightDp.dp.toPx() }

    val positions = remember(names) {
        // Deterministic seeded random so the cloud is stable
        // across recompositions. We seed by list hashCode for
        // extra determinism.
        val seed = names.hashCode().toLong()
        val rng = Random(seed)
        names.map { Pair(rng.nextFloat(), rng.nextFloat()) }
    }

    val infinite = rememberInfiniteTransition(label = "cloud")
    val drift by infinite.animateFloat(
        initialValue = 0f,
        targetValue = 1f,
        animationSpec = infiniteRepeatable(
            animation = tween(durationMillis = 24_000, easing = LinearEasing),
            repeatMode = RepeatMode.Reverse,
        ),
        label = "cloud-drift",
    )
    val driftPx = with(density) { 12.dp.toPx() }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .graphicsLayer { translationY = (drift - 0.5f) * 2f * driftPx },
    ) {
        names.forEachIndexed { i, name ->
            val (fx, fy) = positions[i]
            Text(
                text = name,
                fontFamily = FontFamily.Serif,
                fontSize = 14.sp,
                color = MaterialTheme.colorScheme.onBackground.copy(alpha = 0.08f),
                modifier = Modifier
                    .graphicsLayer {
                        translationX = fx * (widthPx - with(density) { 80.dp.toPx() })
                        translationY = fy * (heightPx - with(density) { 80.dp.toPx() })
                    }
                    .padding(start = 24.dp, top = 48.dp),
            )
        }
    }
}

/** A fixed short list of tarot card names shown in the home
 *  cloud. We embed this static subset (rather than
 *  fetching from the runtime) so the cloud always renders
 *  even if the runtime is offline — the chief app's home
 *  page should look beautiful the moment it opens. */
private val TAROT_CLOUD_NAMES = listOf(
    "愚者", "魔术师", "女祭司", "皇后", "皇帝", "教皇",
    "恋人", "战车", "力量", "隐者", "命运", "正义",
    "吊人", "死神", "节制", "恶魔", "塔", "星星",
    "月亮", "太阳", "审判", "世界",
    "权杖·A", "权杖·国王", "圣杯·A", "圣杯·王后",
    "宝剑·A", "宝剑·国王", "星币·A", "星币·王后",
    "魔术师", "女祭司", "塔", "星星", "月亮", "太阳",
)

// ── Drawing view ─────────────────────────────────────

/** Flip + scale-in animation for one card. Total 1.5s. */
private const val FLIP_MS: Int = 1500

@Composable
private fun CardFlip(initialKey: Any, card: DrawnCard) {
    var rotation by remember(initialKey) { mutableStateOf(0f) }
    var visible by remember(initialKey) { mutableStateOf(false) }
    LaunchedEffect(initialKey) {
        visible = false
        rotation = 0f
        delay(80)
        visible = true
        // Animate 0° → 360° over FLIP_MS via the framework
        // animation primitives. Using animateFloat so we get
        // a smooth ease curve without writing a custom one.
        val target = if (card.reversed) 360f else 360f
        val anim = androidx.compose.animation.core.Animatable(0f)
        anim.animateTo(
            targetValue = target,
            animationSpec = tween(durationMillis = FLIP_MS, easing = LinearEasing),
        )
        rotation = anim.value
    }
    val scale = animateFloatAsState(
        targetValue = if (visible) 1f else 0.6f,
        animationSpec = tween(280),
        label = "card-scale",
    )
    val flippedRotation = if (card.reversed) 180f else 0f
    Image(
        bitmap = remember(card.card.symbol_svg) { renderSvg(card.card.symbol_svg) },
        contentDescription = card.card.name_zh,
        modifier = Modifier
            .size(160.dp, 224.dp)
            .graphicsLayer {
                rotationY = rotation
                rotationX = flippedRotation
                scaleX = scale.value
                scaleY = scale.value
            },
    )
}

@Composable
private fun DrawingView(cards: List<DrawnCard>) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Text(
            text = "洗牌中…",
            fontFamily = FontFamily.Serif,
            fontSize = 14.sp,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        if (cards.size == 1) {
            CardFlip(initialKey = cards.first().card.id, card = cards.first())
        } else {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceEvenly,
            ) {
                cards.forEachIndexed { i, c ->
                    CardFlip(initialKey = i to c.card.id, card = c)
                }
            }
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceEvenly,
            ) {
                cards.forEach { c ->
                    Text(
                        text = c.position,
                        fontFamily = FontFamily.Serif,
                        fontSize = 12.sp,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }
}

// ── Loaded view ─────────────────────────────────────

@Composable
private fun LoadedView(
    cards: List<DrawnCard>,
    onDrawOne: () -> Unit,
    onDrawThree: () -> Unit,
    onClear: () -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(horizontal = 16.dp, vertical = 24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(20.dp),
    ) {
        if (cards.size == 1) {
            SingleCardView(cards.first())
        } else {
            SpreadView(cards)
        }
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            androidx.compose.material3.OutlinedButton(
                onClick = onDrawOne,
                shape = RoundedCornerShape(12.dp),
                modifier = Modifier.weight(1f),
            ) {
                Text("再抽一张", fontFamily = FontFamily.Serif)
            }
            Button(
                onClick = onDrawThree,
                shape = RoundedCornerShape(12.dp),
                colors = ButtonDefaults.buttonColors(
                    containerColor = MaterialTheme.colorScheme.primary,
                ),
                modifier = Modifier.weight(1f),
            ) {
                Text("三卡阵", fontFamily = FontFamily.Serif)
            }
        }
        androidx.compose.material3.TextButton(onClick = onClear) {
            Text("返回首页", fontSize = 12.sp)
        }
    }
}

@Composable
private fun SingleCardView(drawn: DrawnCard) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(16.dp),
    ) {
        Column(
            modifier = Modifier.padding(20.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Image(
                bitmap = remember(drawn.card.symbol_svg) {
                    renderSvg(drawn.card.symbol_svg)
                },
                contentDescription = drawn.card.name_zh,
                modifier = Modifier
                    .size(180.dp, 252.dp)
                    .graphicsLayer {
                        rotationX = if (drawn.reversed) 180f else 0f
                    },
            )
            Text(
                text = drawn.card.name_zh,
                fontFamily = FontFamily.Serif,
                fontSize = 24.sp,
                fontWeight = FontWeight.SemiBold,
                color = MaterialTheme.colorScheme.onSurface,
            )
            Text(
                text = drawn.card.name_pinyin + " · " + drawn.card.name_en,
                fontFamily = FontFamily.Serif,
                fontSize = 12.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            HorizontalDivider()
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Text(
                    text = if (drawn.reversed) "逆位" else "正位",
                    fontFamily = FontFamily.Monospace,
                    fontSize = 11.sp,
                    color = if (drawn.reversed) MaterialTheme.colorScheme.tertiary
                            else MaterialTheme.colorScheme.primary,
                )
                Text(
                    text = drawn.card.arcana.uppercase() + " · " + drawn.card.rank,
                    fontFamily = FontFamily.Monospace,
                    fontSize = 10.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Text(
                text = drawn.meaning,
                fontFamily = FontFamily.Serif,
                fontSize = 14.sp,
                color = MaterialTheme.colorScheme.onSurface,
            )
        }
    }
}

@Composable
private fun SpreadView(cards: List<DrawnCard>) {
    // For 3-card spread, render three cards horizontally.
    // Each card has its own position label (过去/现在/未来).
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .horizontalScroll(rememberScrollState()),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        cards.forEach { drawn ->
            Card(
                modifier = Modifier
                    .width(140.dp),
                shape = RoundedCornerShape(12.dp),
            ) {
                Column(
                    modifier = Modifier.padding(10.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    Text(
                        text = drawn.position,
                        fontFamily = FontFamily.Serif,
                        fontSize = 11.sp,
                        color = MaterialTheme.colorScheme.primary,
                    )
                    Image(
                        bitmap = remember(drawn.card.symbol_svg) {
                            renderSvg(drawn.card.symbol_svg)
                        },
                        contentDescription = drawn.card.name_zh,
                        modifier = Modifier
                            .size(100.dp, 140.dp)
                            .graphicsLayer {
                                rotationX = if (drawn.reversed) 180f else 0f
                            },
                    )
                    Text(
                        text = drawn.card.name_zh,
                        fontFamily = FontFamily.Serif,
                        fontSize = 14.sp,
                        fontWeight = FontWeight.SemiBold,
                        color = MaterialTheme.colorScheme.onSurface,
                    )
                    Text(
                        text = drawn.card.name_pinyin,
                        fontFamily = FontFamily.Serif,
                        fontSize = 10.sp,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Text(
                        text = if (drawn.reversed) "逆位" else "正位",
                        fontFamily = FontFamily.Monospace,
                        fontSize = 9.sp,
                        color = if (drawn.reversed) MaterialTheme.colorScheme.tertiary
                                else MaterialTheme.colorScheme.primary,
                    )
                    Text(
                        text = drawn.meaning,
                        fontFamily = FontFamily.Serif,
                        fontSize = 11.sp,
                        color = MaterialTheme.colorScheme.onSurface,
                    )
                }
            }
        }
    }
}

@Composable
private fun ErrorView(message: String, onRetry: () -> Unit) {
    Column(
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(16.dp),
        modifier = Modifier.padding(24.dp),
    ) {
        Text(
            text = message,
            color = MaterialTheme.colorScheme.error,
            fontFamily = FontFamily.Serif,
        )
        androidx.compose.material3.OutlinedButton(onClick = onRetry) {
            Text("重试")
        }
    }
}

// ── SVG → Bitmap helper ──────────────────────────────
//
// The runtime ships SVGs inline. We render them via
// Android's framework Canvas (no external dep) by walking
// the SVG and painting simple shapes onto a Bitmap. This is
// intentionally minimal — the chairman's spec says "抽卡
// 也要图片" and these hand-drawn symbols are simple enough
// that a one-line-per-shape painter is plenty.
//
// If you need richer SVG support, swap in AndroidSVG or
// Coil's SVG decoder. For v0.1.0 the chief's visual goal is
// "looks like iching-oracle's word cloud but with tarot
// glyphs", not "1:1 svg fidelity".

private fun renderSvg(svg: String): androidx.compose.ui.graphics.ImageBitmap {
    val w = 100
    val h = 140
    val bmp = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
    val canvas = Canvas(bmp)
    canvas.drawColor(android.graphics.Color.TRANSPARENT)
    val paint = Paint().apply {
        color = android.graphics.Color.argb(255, 0, 137, 123) // teal
        strokeWidth = 2f
        isAntiAlias = true
        style = Paint.Style.STROKE
    }
    val fill = Paint().apply {
        color = android.graphics.Color.argb(255, 0, 137, 123)
        isAntiAlias = true
        style = Paint.Style.FILL
    }
    // Parse the inner path/circle/rect SVG primitives and
    // paint them. We support the small subset of SVG used by
    // the runtime's deck: <path d="M x y L x y ..."/>,
    // <circle cx cy r/>, <rect x y w h/>, plus the Z (close).
    val matcher = Regex("""<(path|circle|rect)\s+([^/>]+)/?\s*>""")
    matcher.findAll(svg).forEach { m ->
        val tag = m.groupValues[1]
        val attrs = m.groupValues[2]
        when (tag) {
            "circle" -> {
                val cx = attr(attrs, "cx")?.toFloatOrNull() ?: return@forEach
                val cy = attr(attrs, "cy")?.toFloatOrNull() ?: return@forEach
                val r = attr(attrs, "r")?.toFloatOrNull() ?: return@forEach
                canvas.drawCircle(cx, cy, r, paint)
            }
            "rect" -> {
                val x = attr(attrs, "x")?.toFloatOrNull() ?: 0f
                val y = attr(attrs, "y")?.toFloatOrNull() ?: 0f
                val ww = attr(attrs, "width")?.toFloatOrNull() ?: 0f
                val hh = attr(attrs, "height")?.toFloatOrNull() ?: 0f
                canvas.drawRect(x, y, x + ww, y + hh, paint)
            }
            "path" -> {
                val d = attr(attrs, "d") ?: return@forEach
                val pts = parsePath(d)
                if (pts.size >= 2) {
                    val path = android.graphics.Path()
                    path.moveTo(pts[0].first, pts[0].second)
                    for (i in 1 until pts.size) path.lineTo(pts[i].first, pts[i].second)
                    if (d.contains("Z")) path.close()
                    canvas.drawPath(path, paint)
                }
            }
        }
    }
    // Re-collect small filled shapes by re-scanning for
    // fill="currentColor" (we treat them as filled glyphs
    // — circles and paths the deck marks as filled).
    Regex("""<(circle|path)\s+[^/>]*fill="currentColor"[^/>]*/?\s*>""").findAll(svg).forEach { m ->
        val tag = m.groupValues[1]
        val attrs = m.groupValues[2]
        when (tag) {
            "circle" -> {
                val cx = attr(attrs, "cx")?.toFloatOrNull() ?: return@forEach
                val cy = attr(attrs, "cy")?.toFloatOrNull() ?: return@forEach
                val r = attr(attrs, "r")?.toFloatOrNull() ?: return@forEach
                canvas.drawCircle(cx, cy, r, fill)
            }
            "path" -> {
                val d = attr(attrs, "d") ?: return@forEach
                val pts = parsePath(d)
                if (pts.size >= 2) {
                    val path = android.graphics.Path()
                    path.moveTo(pts[0].first, pts[0].second)
                    for (i in 1 until pts.size) path.lineTo(pts[i].first, pts[i].second)
                    if (d.contains("Z")) path.close()
                    canvas.drawPath(path, fill)
                }
            }
        }
    }
    return bmp.asImageBitmap()
}

private fun attr(attrs: String, name: String): String? =
    Regex("""$name="([^"]+)"""").find(attrs)?.groupValues?.get(1)

/** Parse a path d-attribute into a list of (x, y) points.
 *  Supports M (moveto), L (lineto), Q (quadratic bezier — flat
 *  approximation), and Z (closepath). The runtime's SVGs
 *  are simple enough that we don't need full bezier math. */
private fun parsePath(d: String): List<Pair<Float, Float>> {
    val out = mutableListOf<Pair<Float, Float>>()
    val tokens = Regex("""([MLZ])\s*([\d.\s,]*)""").findAll(d)
    for (t in tokens) {
        val cmd = t.groupValues[1]
        val nums = t.groupValues[2]
            .split(Regex("[\\s,]+"))
            .filter { it.isNotEmpty() }
            .mapNotNull { it.toFloatOrNull() }
        when (cmd) {
            "M", "L" -> if (nums.size >= 2) out.add(nums[0] to nums[1])
            "Q" -> if (nums.size >= 4) {
                // Treat as a straight line to the end point.
                out.add(nums[2] to nums[3])
            }
            "Z" -> { /* close handled by caller */ }
        }
    }
    return out
}
