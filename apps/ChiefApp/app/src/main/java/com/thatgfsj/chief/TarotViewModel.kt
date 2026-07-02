package com.thatgfsj.chief

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.thatgfsj.chief.tarot.TarotCard
import com.thatgfsj.chief.tarot.TarotRepository
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

/**
 * v0.2.1 (event 000076): UI state machine, fully offline.
 *
 *   Initial  → no draw yet, home page
 *   Loaded   → cards drawn, ready for the chairman to tap
 *              each one to flip. Per chairman: '手点翻牌' —
 *              no auto animation, the card sits face-down
 *              until tapped.
 *   Error    → assets/cards.json missing (shouldn't happen
 *              for a built APK; this is a fatal build error
 *              indicator if it does)
 *
 * The previous v0.2.0 had a `Drawing` state (3.1s auto-flip).
 * Removed in v0.2.1: the chairman picked manual flip, so we
 * go Initial → Loaded instantly. The chairman's first tap
 * triggers the 0° → 360° Y-rotation in the UI layer.
 *
 * No network calls. No RuntimeClient. No background services.
 * The Android system can kill this process at any time and
 * the next launch re-reads cards.json in <50ms — that's the
 * 'shotgun mode' chairman picked.
 */
sealed interface TarotUiState {
    data object Initial : TarotUiState
    data class Loaded(val drawn: List<DrawnCard>) : TarotUiState
    data class Error(val message: String) : TarotUiState
}

data class DrawnCard(
    val card: TarotCard,
    val reversed: Boolean,
)

class TarotViewModel(application: Application) : AndroidViewModel(application) {
    private val repo = TarotRepository.getInstance(application)

    private val _state = MutableStateFlow<TarotUiState>(TarotUiState.Initial)
    val state: StateFlow<TarotUiState> = _state.asStateFlow()

    /** Single-card draw. The chief website's "抽卡" button. */
    fun drawOne() {
        viewModelScope.launch {
            val card = repo.drawOne()
            val drawn = listOf(DrawnCard(card, repo.isReversed(card)))
            _state.value = TarotUiState.Loaded(drawn)
        }
    }

    /** Three-card spread: past / present / future. */
    fun drawThree() {
        viewModelScope.launch {
            val cards = repo.drawThree()
            val drawn = cards.map { DrawnCard(it, repo.isReversed(it)) }
            _state.value = TarotUiState.Loaded(drawn)
        }
    }

    /** Reset back to the home page. */
    fun clear() {
        _state.value = TarotUiState.Initial
    }
}
