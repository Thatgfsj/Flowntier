package com.thatgfsj.chief

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.thatgfsj.chief.data.RuntimeClient
import com.thatgfsj.chief.tarot.DrawnCard
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/**
 * v0.1.0 (event 000074): UI state machine for the tarot screen.
 *
 *   Initial  → no draw yet, home page with "点击抽取" button
 *   Drawing  → draw in flight; we already know the card(s) so
 *              the UI can render the flip animation immediately
 *   Loaded   → animation finished, show card + meaning +
 *              "再抽一签" / "三卡阵" buttons
 *   Error    → transport / parse failure; show retry
 *
 * Same shape as IChingOracleScreen but the data path runs
 * through the Flwntier runtime (`/api/tarot/draw`), not a
 * local 64-gua JSON. The chief app's whole reason to exist
 * is to be a Flwntier product surface that *uses* iching-
 * oracle's visual language — it does not duplicate the deck.
 */
sealed interface TarotUiState {
    data object Initial : TarotUiState
    data class Drawing(val cards: List<DrawnCard>) : TarotUiState
    data class Loaded(val cards: List<DrawnCard>, val fadeKey: Int) : TarotUiState
    data class Error(val message: String) : TarotUiState
}

/** Total draw-animation time, in ms. Same 3.1s budget as
 *  iching-oracle's IChingViewModel. */
private const val DRAW_ANIMATION_MS: Long = 3_100L

class TarotViewModel(
    private val client: RuntimeClient = RuntimeClient(),
) : ViewModel() {

    private val _state = MutableStateFlow<TarotUiState>(TarotUiState.Initial)
    val state: StateFlow<TarotUiState> = _state.asStateFlow()

    private val _host = MutableStateFlow("127.0.0.1:8765")
    val host: StateFlow<String> = _host.asStateFlow()

    private val _connected = MutableStateFlow<Boolean?>(null)
    /** null = unknown (haven't pinged yet), true = reachable, false = unreachable. */
    val connected: StateFlow<Boolean?> = _connected.asStateFlow()

    private var fadeCounter = 0

    fun setHost(host: String) {
        _host.value = host
    }

    /** Ping the runtime. Used by the settings pane to verify
     *  the host:port the chairman entered is correct. */
    fun ping() {
        viewModelScope.launch {
            val ok = with(com.thatgfsj.chief.data.RuntimeClient(
                baseUrl = "http://${_host.value}",
            )) { ping() }
            _connected.value = ok
        }
    }

    /**
     * Draw a single card. Public entry point; called by the
     * "点击抽取" button on the home page.
     */
    fun drawOne() {
        viewModelScope.launch {
            _state.value = TarotUiState.Initial
            val resp = client.drawOne()
            if (resp == null || !resp.ok || resp.items.isEmpty()) {
                _state.value = TarotUiState.Error("runtime 不可达或抽卡失败")
                return@launch
            }
            runDrawAnimation(resp.items)
        }
    }

    /**
     * Draw a 3-card past/present/future spread.
     */
    fun drawThree() {
        viewModelScope.launch {
            _state.value = TarotUiState.Initial
            val resp = client.drawThree()
            if (resp == null || !resp.ok || resp.items.size < 3) {
                _state.value = TarotUiState.Error("runtime 不可达或三卡阵失败")
                return@launch
            }
            runDrawAnimation(resp.items)
        }
    }

    /** Reset back to home, clearing the loaded draw. */
    fun clear() {
        _state.value = TarotUiState.Initial
    }

    private suspend fun runDrawAnimation(cards: List<DrawnCard>) {
        _state.value = TarotUiState.Drawing(cards)
        delay(DRAW_ANIMATION_MS)
        fadeCounter += 1
        _state.value = TarotUiState.Loaded(cards, fadeCounter)
    }
}
