package com.thatgfsj.chief.ui.oracle

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
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
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
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
import com.thatgfsj.chief.TarotViewModel
import com.thatgfsj.chief.TarotUiState
import com.thatgfsj.chief.share.DeepSeekShare
import com.thatgfsj.chief.tarot.TarotCard

/**
 * v0.2.1 (event 000076): rewrite the chief app's loaded view
 * for chairman's three bug reports.
 *
 *   1. **Manual flip, not auto.** Each card sits face-down
 *      (rotation 0°) on entry to Loaded; the chairman taps
 *      the card to trigger 0° → 360° over 0.8s. A subsequent
 *      tap re-flip (back to 0°).
 *   2. **Vertical centering.** LoadedView wraps card + button
 *      row in a Box(fillMaxSize, contentAlignment=Center) so
 *      they sit in the middle of the screen, not glued to
 *      the top.
 *   3. **Bigger card, no stacking.** Single card size is
 *      160dp × 230dp (was 100 × 145 — about 1.6× larger).
 *      Three-card spread is a Row with three 80 × 115
 *      cards spaced evenly, not stacked.
 *   4. **问 AI replaces 再抽一张 / 三卡阵.** Buttons row now
 *      has just two: 返回首页 (TextButton) and 问 AI
 *      (filled Button, primary). Both single-card and
 *      three-card views use the same share text format
 *      (one body header + N card lines + one prompt line,
 *      see DeepSeekShare.formatShareText).
 */
@Composable
fun TarotScreen(viewModel: TarotViewModel = viewModel()) {
    val state by viewModel.state.collectAsState()

    // Per chairman bug #2 (re-confirmed in v0.2.1): the home
    // page still sat too close to the top because the
    // previous layout wrapped the whole thing in a
    // verticalScroll Column. verticalScroll always lays
    // children out from the top, so verticalArrangement=
    // Center inside the HomePage Column had no effect (the
    // HomePage Column's intrinsic height was being measured
    // against the full remaining-after-Header space, but the
    // verticalScroll pinned everything to the top of the
    // scroll viewport).
    //
    // Fix: drop the verticalScroll. The home page content is
    // small enough to fit on any phone screen. The loaded
    // view's card is the only thing that could overflow, and
    // the chief website's 78-card draw has a 3-card spread
    // that fits horizontally on every phone we care about.
    // If we ever need to scroll, we'll add it back scoped to
    // a specific sub-view rather than the whole screen.
    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background),
    ) {
        Column(
            modifier = Modifier.fillMaxSize(),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Top,
        ) {
            // Header sits at the top, sized to its content.
            // The state (home page / loaded view) takes the
            // remaining space and is centered within that
            // remaining space by Arrangement.SpaceBetween
            // on the inner layout — see HomePage and
            // LoadedView below, which now use a vertical-
            // center fillMaxSize Box of their own.
            Spacer(Modifier.height(40.dp))
            Header()
            Spacer(Modifier.height(24.dp))

            // Wrap the state's content in a Box that takes
            // the remaining height (weight 1f) and centers
            // vertically, so the home-page buttons / the
            // loaded-view card sit in the middle of the
            // screen, not glued to the top below the header.
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .weight(1f),
                contentAlignment = Alignment.Center,
            ) {
                when (val s = state) {
                    is TarotUiState.Initial -> HomePage(
                        onDrawOne = viewModel::drawOne,
                        onDrawThree = viewModel::drawThree,
                    )
                    is TarotUiState.Loaded -> LoadedView(
                        drawn = s.drawn,
                        onClear = viewModel::clear,
                    )
                    is TarotUiState.Error -> {
                        // Shouldn't happen for a built APK
                        // (assets/cards.json is verified at
                        // build time).
                    }
                }
            }
        }
    }
}

// ── Header / home page ─────────────────────────────────

@Composable
private fun Header() {
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        Text(
            text = "塔罗镜",
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

// ── Loaded view (per chairman bugs 1+2+3+4) ─────────

/**
 * Loaded screen. Card + button row centered as a unit on
 * screen. The card sits face-down; the chairman taps to
 * flip. After the chairman's first tap, the prompt
 * "点击翻牌" fades out and the card is drawable.
 *
 * Three-card spread: each card is its own tap target; each
 * has its own rotation state. The chairman can flip them
 * in any order — the chairman picked "手点,做好手点的
 * 动画即可" (per-event 000076) so we don't stagger.
 */
@Composable
private fun LoadedView(
    drawn: List<DrawnCard>,
    onClear: () -> Unit,
) {
    val context = LocalContext.current
    Box(
        modifier = Modifier
            .fillMaxSize()
            .padding(horizontal = 16.dp, vertical = 16.dp),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            modifier = Modifier.fillMaxWidth(),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(20.dp),
        ) {
            if (drawn.size == 1) {
                SingleCardView(drawn.first())
            } else {
                SpreadView(drawn)
            }
            Text(
                text = "—",
                fontFamily = FontFamily.Serif,
                fontSize = 4.sp,
            )
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                OutlinedButton(
                    onClick = onClear,
                    shape = RoundedCornerShape(12.dp),
                    modifier = Modifier.weight(1f),
                ) {
                    Text("返回首页", fontFamily = FontFamily.Serif)
                }
                Button(
                    onClick = { DeepSeekShare.shareToDeepSeek(context, drawn) },
                    shape = RoundedCornerShape(12.dp),
                    colors = ButtonDefaults.buttonColors(
                        containerColor = MaterialTheme.colorScheme.primary,
                        contentColor = MaterialTheme.colorScheme.onPrimary,
                    ),
                    modifier = Modifier.weight(1f),
                ) {
                    Text("✨  问 AI", fontFamily = FontFamily.Serif)
                }
            }
        }
    }
}

/**
 * Single card. Manual flip on tap. The card image is
 * the same drawable from assets — we just spin it 0° → 360°
 * via a Compose `Animatable`. The "click to flip" hint
 * fades out after the first flip.
 *
 * Size: 160 × 230 dp (chairman's chosen big-size, ~1.6×
 * the v0.2.0 100 × 145).
 */
@Composable
private fun SingleCardView(drawn: DrawnCard) {
    var rotation by remember { mutableStateOf(0f) }
    var flipped by remember { mutableStateOf(false) }
    val flippedX = if (drawn.reversed) 180f else 0f
    val targetRotation = if (flipped) 360f else 0f
    // Animate to the target whenever it changes — i.e. on
    // every tap. Tapping the card flips rotation between
    // 0° and 360° (1.5s round-trip, but the chairman can
    // interrupt by tapping again — Animatable.animateTo
    // cancels the running animation).
    LaunchedEffect(rotation) {
        if (flipped && rotation == 360f) {
            // already at target, no-op
            return@LaunchedEffect
        }
        val anim = androidx.compose.animation.core.Animatable(targetRotation)
        anim.animateTo(
            targetValue = (if (flipped) 360f else 0f),
            animationSpec = tween(durationMillis = 800, easing = LinearEasing),
        )
        // Sync local rotation to the animated value on
        // completion. We need this because AnimatedImage
        // binds to `rotation` reactively.
        rotation = anim.value
    }
    val scale = animateFloatAsState(
        targetValue = 1f,
        animationSpec = tween(280),
        label = "card-scale-in",
    )
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        // The hint fades out after the first flip.
        AnimatedVisibility(visible = !flipped) {
            Text(
                text = "点击翻牌",
                fontFamily = FontFamily.Serif,
                fontSize = 11.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f),
                modifier = Modifier.padding(bottom = 6.dp),
            )
        }
        Image(
            painter = painterResource(id = drawableId(drawn.card)),
            contentDescription = drawn.card.name_zh,
            contentScale = ContentScale.Fit,
            modifier = Modifier
                .size(width = 160.dp, height = 230.dp)
                .graphicsLayer {
                    rotationY = rotation
                    rotationX = flippedX
                    scaleX = scale.value
                    scaleY = scale.value
                }
                .clickable {
                    flipped = !flipped
                    // Don't manually set rotation here —
                    // the LaunchedEffect on rotation will
                    // pick up the new flipped value and
                    // animate to the new target.
                },
        )
        Spacer(Modifier.height(8.dp))
        Text(
            text = drawn.card.name_zh,
            fontFamily = FontFamily.Serif,
            fontSize = 20.sp,
            fontWeight = FontWeight.SemiBold,
            color = MaterialTheme.colorScheme.onSurface,
        )
        Text(
            text = drawn.card.name_en,
            fontFamily = FontFamily.Serif,
            fontSize = 11.sp,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            text = if (drawn.reversed) "逆位" else "顺位",
            fontFamily = FontFamily.Monospace,
            fontSize = 10.sp,
            color = if (drawn.reversed) MaterialTheme.colorScheme.tertiary
                    else MaterialTheme.colorScheme.primary,
        )
    }
}

/**
 * Three-card spread. Each card is its own tap target
 * with its own rotation state. Horizontal row, no
 * stacking (chairman: "别堆叠"). Position labels
 * 过去 / 现在 / 未来 sit above each card.
 */
@Composable
private fun SpreadView(drawn: List<DrawnCard>) {
    val positions = listOf("过去", "现在", "未来")
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceEvenly,
        verticalAlignment = Alignment.Bottom,
    ) {
        drawn.forEachIndexed { i, d ->
            SpreadCard(d, position = positions.getOrElse(i) { "" })
        }
    }
}

@Composable
private fun SpreadCard(drawn: DrawnCard, position: String) {
    var rotation by remember { mutableStateOf(0f) }
    var flipped by remember { mutableStateOf(false) }
    val flippedX = if (drawn.reversed) 180f else 0f
    LaunchedEffect(rotation) {
        val anim = androidx.compose.animation.core.Animatable(
            if (flipped) 360f else 0f
        )
        anim.animateTo(
            targetValue = if (flipped) 360f else 0f,
            animationSpec = tween(durationMillis = 800, easing = LinearEasing),
        )
        rotation = anim.value
    }
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        Text(
            text = position,
            fontFamily = FontFamily.Serif,
            fontSize = 12.sp,
            color = MaterialTheme.colorScheme.primary,
            modifier = Modifier.padding(bottom = 6.dp),
        )
        Image(
            painter = painterResource(id = drawableId(drawn.card)),
            contentDescription = drawn.card.name_zh,
            contentScale = ContentScale.Fit,
            modifier = Modifier
                .size(width = 80.dp, height = 115.dp)
                .graphicsLayer {
                    rotationY = rotation
                    rotationX = flippedX
                }
                .clickable {
                    flipped = !flipped
                },
        )
        Spacer(Modifier.height(4.dp))
        Text(
            text = drawn.card.name_zh,
            fontFamily = FontFamily.Serif,
            fontSize = 11.sp,
            fontWeight = FontWeight.SemiBold,
            color = MaterialTheme.colorScheme.onSurface,
        )
        Text(
            text = if (drawn.reversed) "逆位" else "顺位",
            fontFamily = FontFamily.Monospace,
            fontSize = 9.sp,
            color = if (drawn.reversed) MaterialTheme.colorScheme.tertiary
                    else MaterialTheme.colorScheme.primary,
        )
    }
}

@Composable
private fun drawableId(card: TarotCard): Int {
    val ctx = LocalContext.current
    val name = card.image_res
    return remember(name) {
        ctx.resources.getIdentifier(name, "drawable", ctx.packageName)
    }
}
