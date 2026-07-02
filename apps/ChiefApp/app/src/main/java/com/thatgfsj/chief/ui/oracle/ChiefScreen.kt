package com.thatgfsj.chief.ui.oracle

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.thatgfsj.chief.ChiefViewModel
import com.thatgfsj.chief.RunningState

// The 8 phases from history/PROJECT_SPEC.md (event 000068
// orchestrator). The phase names here match the orchestrator's
// PHASES const exactly — the runtime sends these strings on
// PhaseTransition events and the status endpoint echoes
// them; we match phase to position in this list to light up
// the right dot.
private val PHASES = listOf(
    "requirement" to "1-需求",
    "plan" to "2-规划",
    "plan-review" to "3-计划审核",
    "dispatch" to "4-派发",
    "develop" to "5-开发",
    "final-review" to "6-终审",
    "repair" to "7-修复",
    "delivery" to "8-交付",
)

/**
 * Single-screen chief console. Mirrors IChingOracle's
 * IChingOracleScreen layout (centered content, serif
 * headings, ink-and-paper background) but adapted to the
 * Flowntier workflow semantics: a task input, a phase
 * timeline, and a delivery summary.
 */
@Composable
fun ChiefScreen(viewModel: ChiefViewModel) {
    val state by viewModel.state.collectAsState()
    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background)
            .padding(16.dp),
    ) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState()),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Header()
            Spacer(Modifier.height(16.dp))
            HostField(
                host = state.host,
                onChange = viewModel::setHost,
            )
            Spacer(Modifier.height(8.dp))
            TaskField(
                task = state.task,
                sending = state.sending,
                onChange = viewModel::setTask,
                onSend = viewModel::send,
            )
            Spacer(Modifier.height(16.dp))
            state.lastError?.let { err ->
                ErrorBanner(err)
                Spacer(Modifier.height(12.dp))
            }
            state.running?.let { running ->
                PhaseTimeline(running)
                Spacer(Modifier.height(12.dp))
                SummaryCard(running)
            } ?: EmptyHint()
        }
    }
}

@Composable
private fun Header() {
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        Text(
            text = "主理",
            fontFamily = FontFamily.Serif,
            fontWeight = FontWeight.SemiBold,
            fontSize = 36.sp,
            color = MaterialTheme.colorScheme.primary,
        )
        Spacer(Modifier.height(2.dp))
        Text(
            text = "Flowntier 任务调度员",
            fontFamily = FontFamily.Serif,
            fontSize = 12.sp,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun HostField(host: String, onChange: (String) -> Unit) {
    OutlinedTextField(
        value = host,
        onValueChange = onChange,
        label = { Text("runtime host:port", fontSize = 11.sp) },
        singleLine = true,
        modifier = Modifier.fillMaxWidth(0.9f),
        textStyle = MaterialTheme.typography.bodyMedium.copy(
            fontFamily = FontFamily.Monospace,
        ),
    )
}

@Composable
private fun TaskField(
    task: String,
    sending: Boolean,
    onChange: (String) -> Unit,
    onSend: () -> Unit,
) {
    Column(modifier = Modifier.fillMaxWidth(0.9f)) {
        OutlinedTextField(
            value = task,
            onValueChange = onChange,
            label = { Text("任务", fontSize = 11.sp) },
            placeholder = {
                Text(
                    "例:做一个塔罗牌 app,完整 78 张 + 三卡阵 + 翻牌动效",
                    fontSize = 12.sp,
                )
            },
            minLines = 3,
            maxLines = 6,
            enabled = !sending,
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(8.dp))
        Button(
            onClick = onSend,
            enabled = !sending && task.isNotBlank(),
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(24.dp),
        ) {
            Text(
                text = if (sending) "发送中…" else "派发",
                fontFamily = FontFamily.Serif,
                fontSize = 14.sp,
                modifier = Modifier.padding(vertical = 4.dp),
            )
        }
    }
}

@Composable
private fun PhaseTimeline(running: RunningState) {
    // The runtime's phase string looks like "5-develop" or
    // '"5-develop"' (Debug-printed). Match on substring.
    val rawPhase = running.phase.trim('"')
    val phaseName = rawPhase
        .substringAfter('-', missingDelimiterValue = rawPhase)
    val currentIdx = PHASES.indexOfFirst { (name, _) ->
        phaseName.contains(name, ignoreCase = true)
    }
    val activeIdx = if (currentIdx < 0) 0 else currentIdx

    Card(
        modifier = Modifier
            .fillMaxWidth(0.9f),
        shape = RoundedCornerShape(12.dp),
    ) {
        Column(modifier = Modifier.padding(16.dp)) {
            Text(
                text = "PhaseTimeline · ${running.wfId.take(20)}…",
                fontFamily = FontFamily.Monospace,
                fontSize = 10.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(10.dp))
            if (running.tasksTotal > 0) {
                LinearProgressIndicator(
                    progress = {
                        running.tasksDone.toFloat() /
                            running.tasksTotal.coerceAtLeast(1)
                    },
                    modifier = Modifier.fillMaxWidth(),
                )
                Spacer(Modifier.height(4.dp))
                Text(
                    text = "任务 ${running.tasksDone} / ${running.tasksTotal}",
                    fontSize = 10.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(12.dp))
            }
            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                PHASES.forEachIndexed { i, (name, label) ->
                    PhaseRow(
                        label = label,
                        state = when {
                            i < activeIdx -> PhaseState.Done
                            i == activeIdx -> PhaseState.Active
                            else -> PhaseState.Pending
                        },
                    )
                }
            }
        }
    }
}

private enum class PhaseState { Done, Active, Pending }

@Composable
private fun PhaseRow(label: String, state: PhaseState) {
    val (dot, color) = when (state) {
        PhaseState.Done -> "●" to MaterialTheme.colorScheme.primary
        PhaseState.Active -> "◉" to MaterialTheme.colorScheme.primary
        PhaseState.Pending -> "○" to MaterialTheme.colorScheme.onSurfaceVariant
    }
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text(
            text = dot,
            color = color,
            fontSize = 14.sp,
            modifier = Modifier.width(20.dp),
            textAlign = TextAlign.Center,
        )
        Text(
            text = label,
            fontFamily = FontFamily.Serif,
            fontSize = 13.sp,
            color = if (state == PhaseState.Pending)
                MaterialTheme.colorScheme.onSurfaceVariant
            else MaterialTheme.colorScheme.onSurface,
        )
    }
}

@Composable
private fun SummaryCard(running: RunningState) {
    val isTerminal = running.phase.contains("done", ignoreCase = true)
        || running.phase.contains("delivery", ignoreCase = true)
    Card(
        modifier = Modifier
            .fillMaxWidth(0.9f),
        shape = RoundedCornerShape(12.dp),
    ) {
        Column(modifier = Modifier.padding(16.dp)) {
            Text(
                text = if (isTerminal) "📜 交付总结" else "📜 当前进度",
                fontFamily = FontFamily.Serif,
                fontSize = 12.sp,
                color = MaterialTheme.colorScheme.primary,
            )
            Spacer(Modifier.height(6.dp))
            HorizontalDivider()
            Spacer(Modifier.height(8.dp))
            Text(
                text = running.summary?.takeIf { it.isNotBlank() }
                    ?: "(chief 还在写,稍等…)",
                fontFamily = FontFamily.Serif,
                fontSize = 12.sp,
                color = MaterialTheme.colorScheme.onSurface,
            )
        }
    }
}

@Composable
private fun ErrorBanner(message: String) {
    Card(
        modifier = Modifier.fillMaxWidth(0.9f),
        shape = RoundedCornerShape(8.dp),
    ) {
        Text(
            text = "⚠ " + message,
            modifier = Modifier.padding(12.dp),
            color = MaterialTheme.colorScheme.error,
            fontSize = 12.sp,
        )
    }
}

@Composable
private fun EmptyHint() {
    Spacer(Modifier.height(40.dp))
    Text(
        text = "派发任务后,这里会显示 8 阶段进度 + chief 最终交付总结。",
        fontFamily = FontFamily.Serif,
        fontSize = 11.sp,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        textAlign = TextAlign.Center,
        modifier = Modifier
            .fillMaxWidth(0.8f)
            .padding(8.dp),
    )
}
