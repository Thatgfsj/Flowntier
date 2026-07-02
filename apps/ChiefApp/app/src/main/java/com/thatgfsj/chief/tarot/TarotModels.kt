package com.thatgfsj.chief.tarot

import kotlinx.serialization.Serializable

/**
 * v0.1.0 (event 000074): the tarot data model mirrors the
 * wire shape from POST /api/tarot/draw on the Flwntier runtime.
 * The runtime's Rust `tarot.rs` module produces these; the
 * chief app consumes them and renders 翻牌 + 解读 in
 * iching-oracle's visual language.
 */

@Serializable
data class TarotCard(
    val id: Int,
    val arcana: String,           // "major" | "minor"
    val suit: String? = null,     // "wands" | "cups" | "swords" | "pentacles" | null
    val rank: String,             // "0".."21" for major; "ace".."king" for minor
    val name_zh: String,
    val name_pinyin: String,
    val name_en: String,
    val symbol_svg: String,
    val upright_meaning: String,
    val reversed_meaning: String,
)

@Serializable
data class DrawnCard(
    val position: String,
    val reversed: Boolean,
    val meaning: String,
    val card: TarotCard,
)

@Serializable
data class TarotDrawResponse(
    val ok: Boolean,
    val spread: String,
    val count: Int,
    val drawn_at_ms: Long,
    val items: List<DrawnCard>,
)

@Serializable
data class TarotListCard(
    val id: Int,
    val arcana: String,
    val suit: String? = null,
    val rank: String,
    val name_zh: String,
    val name_pinyin: String,
    val name_en: String,
)

@Serializable
data class TarotListResponse(
    val ok: Boolean,
    val count: Int,
    val cards: List<TarotListCard>,
)
