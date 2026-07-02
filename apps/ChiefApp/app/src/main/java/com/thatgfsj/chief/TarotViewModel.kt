package com.thatgfsj.chief

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.thatgfsj.chief.tarot.TarotCard
import com.thatgfsj.chief.tarot.TarotRepository
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

/**
 * v0.2.0 (event 000075): UI state machine, fully offline.
 *
 *   Initial  → no draw yet, home page
 *   Drawing  → cards known, animation in flight (~3.1s)
 *   Loaded   → animation done, render the card(s)
 *   Error    → assets/cards.json missing (shouldn't happen
 *              for a built APK; this is a fatal build error
 *              indicator if it does)
 *
 * No network calls. No RuntimeClient. No background services.
 * The Android system can kill this process at any time and
 * the next launch re-reads cards.json in <50ms — that's the
 * 'shotgun mode' chairman picked.
 */
sealed interface TarotUiState {
    data object Initial : TarotUiState
    data class Drawing(val drawn: List<DrawnCard>) : TarotUiState
    data class Loaded(val drawn: List<DrawnCard>, val fadeKey: Int) : TarotUiState
    data class Error(val message: String) : TarotUiState
}

data class DrawnCard(
    val card: TarotCard,
    val reversed: Boolean,
)

/** Total draw-animation time, in ms. Matches iching-oracle's
 *  3.1s budget so the chief app *feels* like the same product. */
private const val DRAW_ANIMATION_MS: Long = 3_100L

class TarotViewModel(application: Application) : AndroidViewModel(application) {
    private val repo = TarotRepository.getInstance(application)

    private val _state = MutableStateFlow<TarotUiState>(TarotUiState.Initial)
    val state: StateFlow<TarotUiState> = _state.asStateFlow()

    private var fadeCounter = 0

    /** Single-card draw. The chief website's "抽卡" button. */
    fun drawOne() {
        viewModelScope.launch {
            val card = repo.drawOne()
            val drawn = listOf(DrawnCard(card, repo.isReversed(card)))
            runDrawAnimation(drawn)
        }
    }

    /** Three-card spread: past / present / future. */
    fun drawThree() {
        viewModelScope.launch {
            val cards = repo.drawThree()
            val drawn = cards.map { DrawnCard(it, repo.isReversed(it)) }
            runDrawAnimation(drawn)
        }
    }

    /** Reset back to the home page. */
    fun clear() {
        _state.value = TarotUiState.Initial
    }

    private suspend fun runDrawAnimation(drawn: List<DrawnCard>) {
        _state.value = TarotUiState.Drawing(drawn)
        delay(DRAW_ANIMATION_MS)
        fadeCounter += 1
        _state.value = TarotUiState.Loaded(drawn, fadeCounter)
    }
}
