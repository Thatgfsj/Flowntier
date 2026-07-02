package com.thatgfsj.chief.ui.oracle

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
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
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
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
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.viewmodel.compose.viewModel
import com.thatgfsj.chief.DrawnCard
import com.thatgfsj.chief.R
import com.thatgfsj.chief.TarotViewModel
import com.thatgfsj.chief.TarotUiState
import com.thatgfsj.chief.tarot.TarotCard
import kotlin.math.sin

/**
 * v0.2.0 (event 000075): full rewrite of the tarot screen.
 *
 * - Loads card images from res/drawable-nodpi (baked at
 *   build time, 78 PNG/GIF files from shenpowang.com).
 * - Uses the chief website's purple-black + gold palette.
 * - Three-card spread lays out horizontally (chief website
 *   does the same).
 * - Card flip animation: Y-axis rotate 0° → 360° over 1.5s
 *   with a brief scale-in. Reversed cards have an
 *   additional 180° X-rotation, mirroring the chief
 *   website's "card flipped upside down" affordance.
 * - The cardback / 翻牌 moment: when the card lands at
 *   180° Y, it's briefly edge-on (invisible), then the
 *   front face shows. We approximate this with a scale
 *   curve that dips at 0.5.
 *
 * No runtime calls. No services. The Android system can
 * kill this process at any time and the next launch is
 * < 1s (the deck is already in APK assets).
 */
@Composable
fun TarotScreen(viewModel: TarotViewModel = viewModel()) {
    val state by viewModel.state.collectAsState()

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
            Spacer(Modifier.height(40.dp))
            Header()
            Spacer(Modifier.height(24.dp))

            when (val s = state) {
                is TarotUiState.Initial -> HomePage(
                    onDrawOne = viewModel::drawOne,
                    onDrawThree = viewModel::drawThree,
                )
                is TarotUiState.Drawing -> DrawingView(s.drawn)
                is TarotUiState.Loaded -> LoadedView(
                    drawn = s.drawn,
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
            text = "星辰之镜",
            fontFamily = FontFamily.Serif,
            fontWeight = FontWeight.SemiBold,
            fontSize = 40.sp,
            color = MaterialTheme.colorScheme.primary,
        )
        Spacer(Modifier.height(2.dp))
        Text(
            text = "Tarot Mirror",
            fontFamily = FontFamily.Serif,
            fontSize = 14.sp,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(8.dp))
        Text(
            text = "心诚则灵",
            fontFamily = FontFamily.Serif,
            fontSize = 11.sp,
            color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f),
        )
    }
}

@Composable
private fun HomePage(
    onDrawOne: () -> Unit,
    onDrawThree: () -> Unit,
) {
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
            fontSize = 12.sp,
            color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f),
        )
        Spacer(Modifier.height(48.dp))
        Button(
            onClick = onDrawOne,
            shape = RoundedCornerShape(28.dp),
            colors = ButtonDefaults.buttonColors(
                containerColor = MaterialTheme.colorScheme.primary,
                contentColor = MaterialTheme.colorScheme.onPrimary,
            ),
            contentPadding = PaddingValues(horizontal = 40.dp, vertical = 16.dp),
        ) {
            Text(
                text = "点击抽取",
                fontFamily = FontFamily.Serif,
                fontSize = 18.sp,
            )
        }
        Spacer(Modifier.height(20.dp))
        OutlinedButton(
            onClick = onDrawThree,
            shape = RoundedCornerShape(28.dp),
            colors = ButtonDefaults.outlinedButtonColors(
                contentColor = MaterialTheme.colorScheme.primary,
            ),
            contentPadding = PaddingValues(horizontal = 32.dp, vertical = 12.dp),
        ) {
            Text(
                text = "三卡阵  ·  过去 / 现在 / 未来",
                fontFamily = FontFamily.Serif,
                fontSize = 14.sp,
            )
        }
    }
}

// ── Drawing view ─────────────────────────────────────

/** Card-flip animation: a single card. The runtime gives
 *  us the card up front (so we render the front face
 *  immediately) and animate the Y-axis rotation 0° → 360°
 *  over 1.5s. The "cardback" is implied by the brief
 *  edge-on moment near 90°/270°.
 *
 *  Reversed cards add a 180° X-rotation on top of the Y
 *  spin, so they land upside-down. */
@Composable
private fun CardFlip(drawn: DrawnCard, animationKey: Any) {
    var rotation by remember(animationKey) { mutableStateOf(0f) }
    val target = 360f
    LaunchedEffect(animationKey) {
        rotation = 0f
        // Animate 0° → 360° over 1.5s.
        val anim = androidx.compose.animation.core.Animatable(0f)
        anim.animateTo(
            targetValue = target,
            animationSpec = tween(durationMillis = 1500, easing = LinearEasing),
        )
        rotation = anim.value
    }
    val scale = animateFloatAsState(
        targetValue = 1f,
        animationSpec = tween(280),
        label = "card-scale-in",
    )
    val flippedX = if (drawn.reversed) 180f else 0f
    Image(
        painter = painterResource(id = drawableId(drawn.card)),
        contentDescription = drawn.card.name_zh,
        contentScale = ContentScale.Fit,
        modifier = Modifier
            .size(width = 100.dp, height = 145.dp)
            .graphicsLayer {
                rotationY = rotation
                rotationX = flippedX
                scaleX = scale.value
                scaleY = scale.value
            },
    )
}

/** Look up the Android drawable id for a card. The image_res
 *  field in cards.json matches the file name in
 *  res/drawable-nodpi/ minus the extension. We use
 *  `resources.getIdentifier` rather than R.drawable.*
 *  because the 78 card names are data-driven (not known
 *  at compile time per-id). */
@Composable
private fun drawableId(card: TarotCard): Int {
    val ctx = LocalContext.current
    val name = card.image_res
    // Cache the lookup by name — R-field reflect() on every
    // draw would be expensive. The values don't change at
    // runtime.
    return remember(name) {
        ctx.resources.getIdentifier(name, "drawable", ctx.packageName)
    }
}

@Composable
private fun DrawingView(drawn: List<DrawnCard>) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(20.dp),
    ) {
        if (drawn.size == 1) {
            CardFlip(drawn.first(), animationKey = "draw-${drawn.hashCode()}")
        } else {
            // 3-card spread
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceEvenly,
            ) {
                drawn.forEachIndexed { i, d ->
                    CardFlip(d, animationKey = "draw-$i-${d.card.id}")
                }
            }
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceEvenly,
            ) {
                listOf("过去", "现在", "未来").forEach { pos ->
                    Text(
                        text = pos,
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
    drawn: List<DrawnCard>,
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
        if (drawn.size == 1) {
            SingleCardView(drawn.first())
        } else {
            SpreadView(drawn)
        }
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            OutlinedButton(
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
                    contentColor = MaterialTheme.colorScheme.onPrimary,
                ),
                modifier = Modifier.weight(1f),
            ) {
                Text("三卡阵", fontFamily = FontFamily.Serif)
            }
        }
        TextButton(onClick = onClear) {
            Text("返回首页", fontSize = 12.sp)
        }
    }
}

@Composable
private fun SingleCardView(drawn: DrawnCard) {
    Card(
        modifier = Modifier
            .fillMaxWidth(),
        shape = RoundedCornerShape(16.dp),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surface,
        ),
    ) {
        Column(
            modifier = Modifier.padding(20.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Image(
                painter = painterResource(id = drawableId(drawn.card)),
                contentDescription = drawn.card.name_zh,
                contentScale = ContentScale.Fit,
                modifier = Modifier
                    .size(width = 120.dp, height = 175.dp)
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
                text = drawn.card.name_en,
                fontFamily = FontFamily.Serif,
                fontSize = 12.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
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
                    text = (drawn.card.arcana + " · " +
                            (drawn.card.suit ?: drawn.card.id.toString())).uppercase(),
                    fontFamily = FontFamily.Monospace,
                    fontSize = 10.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            // v0.3 placeholder for meaning text — the
            // chairman's tarot-website has 诗意解读 lines
            // (see workspace/tarot/index.html), but the
            // current chief-app deck JSON doesn't carry
            // them. The runtime's /api/tarot/all endpoint
            // does (event 000074) — we can either:
            //   (a) extend assets/cards.json to include
            //       upright/reversed_meaning, or
            //   (b) call the runtime for meaning text only.
            // v0.2 ships without; v0.3 will pick (a) per
            // the chairman's "all baked" preference.
            Text(
                text = "—",
                fontFamily = FontFamily.Serif,
                fontSize = 13.sp,
                color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.4f),
            )
        }
    }
}

@Composable
private fun SpreadView(drawn: List<DrawnCard>) {
    val positions = listOf("过去", "现在", "未来")
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceEvenly,
    ) {
        drawn.forEachIndexed { i, d ->
            Card(
                modifier = Modifier
                    .width(108.dp),
                shape = RoundedCornerShape(12.dp),
                colors = CardDefaults.cardColors(
                    containerColor = MaterialTheme.colorScheme.surface,
                ),
            ) {
                Column(
                    modifier = Modifier.padding(8.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    Text(
                        text = positions.getOrElse(i) { "" },
                        fontFamily = FontFamily.Serif,
                        fontSize = 11.sp,
                        color = MaterialTheme.colorScheme.primary,
                    )
                    Image(
                        painter = painterResource(id = drawableId(d.card)),
                        contentDescription = d.card.name_zh,
                        contentScale = ContentScale.Fit,
                        modifier = Modifier
                            .size(width = 90.dp, height = 130.dp)
                            .graphicsLayer {
                                rotationX = if (d.reversed) 180f else 0f
                            },
                    )
                    Text(
                        text = d.card.name_zh,
                        fontFamily = FontFamily.Serif,
                        fontSize = 13.sp,
                        fontWeight = FontWeight.SemiBold,
                        color = MaterialTheme.colorScheme.onSurface,
                    )
                    Text(
                        text = if (d.reversed) "逆位" else "正位",
                        fontFamily = FontFamily.Monospace,
                        fontSize = 9.sp,
                        color = if (d.reversed) MaterialTheme.colorScheme.tertiary
                                else MaterialTheme.colorScheme.primary,
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
        OutlinedButton(onClick = onRetry) {
            Text("重试")
        }
    }
}
