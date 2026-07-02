package com.thatgfsj.chief.tarot

import android.content.Context
import kotlinx.serialization.json.Json
import java.io.IOException

/**
 * v0.2.0 (event 000075): loads the 78-card deck from
 * `assets/cards.json` once and caches it in memory for the
 * lifetime of the chief app process. The chief app is
 * shotgun-mode (no background) so a per-process cache is
 * sufficient — when Android kills the process, the next
 * launch re-reads the asset in well under 50 ms.
 *
 * Mirrors iching-oracle's `HexagramRepository` pattern
 * (getInstance + Application.onCreate), but since chief
 * has no Application subclass (single-activity host), the
 * cache lives on first call to `getInstance(context)`.
 */
class TarotRepository private constructor(
    val cards: List<TarotCard>,
) {
    companion object {
        @Volatile
        private var INSTANCE: TarotRepository? = null

        fun getInstance(context: Context): TarotRepository {
            return INSTANCE ?: synchronized(this) {
                INSTANCE ?: load(context.applicationContext).also { INSTANCE = it }
            }
        }

        private fun load(context: Context): TarotRepository {
            val json = context.assets.open("cards.json").use { stream ->
                stream.bufferedReader().readText()
            }
            val deck = Json { ignoreUnknownKeys = true }
                .decodeFromString(TarotDeck.serializer(), json)
            require(deck.cards.size == 78) {
                "cards.json must contain 78 entries, found ${deck.cards.size}"
            }
            return TarotRepository(deck.cards)
        }
    }

    /**
     * Pick one card uniformly at random. Uses kotlin.random
     * (XorWow, fine for a visual draw — not security).
     */
    fun drawOne(): TarotCard = cards.random()

    /**
     * Three-card spread: past / present / future.
     * Each call is independent random — same card can
     * appear multiple times in a single spread (matches
     * iching-oracle's behaviour).
     */
    fun drawThree(): List<TarotCard> = List(3) { cards.random() }

    /** 50/50 reversed toggle per card (mirrors the chief
     *  website's `reversed` flag). */
    fun isReversed(card: TarotCard): Boolean = kotlin.random.Random.nextBoolean()
}
