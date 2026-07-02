package com.thatgfsj.chief

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.thatgfsj.chief.data.RuntimeClient
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/**
 * State machine for the chief app's single screen.
 *
 *   Idle                 → user types task, hits send
 *   Sending              → POST /api/run_workflow in flight
 *   Running(wfId, ...)   → orchestrator is running, poll loop
 *                            is active, phase animates
 *   Done(wfId, summary)  → terminal — chief delivered the
 *                            user-facing summary
 *   Error(message)       → transport / parse failure
 *
 * The state machine is intentionally small. The 8-phase
 * progress lives inside `Running.phase` and the per-phase
 * status string from /api/workflow/{wf_id}/status.
 */
data class ChiefUiState(
    val host: String = "192.168.1.10:8765",
    val task: String = "",
    val sending: Boolean = false,
    val running: RunningState? = null,
    val lastError: String? = null,
)

data class RunningState(
    val wfId: String,
    val phase: String,
    val tasksDone: Int,
    val tasksTotal: Int,
    val summary: String? = null,
)

class ChiefViewModel(
    private val client: RuntimeClient = RuntimeClient(),
) : ViewModel() {

    private val _state = MutableStateFlow(ChiefUiState())
    val state: StateFlow<ChiefUiState> = _state.asStateFlow()

    private var pollJob: Job? = null

    fun setHost(host: String) {
        _state.update { it.copy(host = host) }
    }

    fun setTask(task: String) {
        _state.update { it.copy(task = task) }
    }

    fun send() {
        val s = _state.value
        if (s.sending || s.task.isBlank()) return
        _state.update { it.copy(sending = true, lastError = null, running = null) }
        viewModelScope.launch {
            try {
                val start = client.startWorkflow(s.task)
                _state.update {
                    it.copy(
                        sending = false,
                        running = RunningState(
                            wfId = start.wfId,
                            phase = "starting",
                            tasksDone = 0,
                            tasksTotal = 0,
                        ),
                    )
                }
                startPolling(start.wfId)
            } catch (e: Exception) {
                _state.update {
                    it.copy(
                        sending = false,
                        lastError = e.message ?: e::class.java.simpleName,
                    )
                }
            }
        }
    }

    private fun startPolling(wfId: String) {
        pollJob?.cancel()
        pollJob = viewModelScope.launch {
            while (true) {
                val s = client.getStatus(wfId)
                if (s == null) {
                    delay(2_000)
                    continue
                }
                val stillRunning = s.status.equals("active", ignoreCase = true)
                        || s.status.equals("running", ignoreCase = true)
                _state.update {
                    it.copy(
                        running = RunningState(
                            wfId = s.wfId,
                            phase = s.phase,
                            tasksDone = s.tasksDone,
                            tasksTotal = s.tasksTotal,
                            summary = s.summary,
                        ),
                    )
                }
                if (!stillRunning) {
                    // Terminal state — leave `running` populated
                    // so the screen shows the final summary.
                    return@launch
                }
                delay(2_000)
            }
        }
    }

    fun clear() {
        pollJob?.cancel()
        _state.update { it.copy(running = null, lastError = null) }
    }
}

