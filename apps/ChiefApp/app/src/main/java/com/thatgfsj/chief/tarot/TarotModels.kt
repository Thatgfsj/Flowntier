package com.thatgfsj.chief.tarot

import kotlinx.serialization.Serializable

/**
 * v0.2.0 (event 000075): the tarot data model is now
 * fully offline. The 78-card deck lives in
 * `assets/cards.json` (baked into the APK at build time),
 * the 78 card images live in `res/drawable-nodpi/<name>.jpg`.
 *
 * Field set:
 *   id        0..77 (0 = 愚者 / The Fool)
 *   name_zh   中文名 (e.g. 愚者)
 *   name_en   English name (e.g. The Fool)
 *   image_res drawable resource name, lowercase + underscores
 *             (e.g. "the_fool" → R.drawable.the_fool)
 *   arcana    "major" | "minor"
 *   suit      "wands" | "cups" | "swords" | "pentacles" | null
 *
 * The model is intentionally minimal: no SVG, no
 * upright_meaning / reversed_meaning strings — the
 * chairman's directive for v0.2 was 'app 不能依赖后端,
 * 用 assets 抽牌'. We carry image + name only; meaning
 * text comes from a future chief task (the chairman
 * flagged this as a v0.3 item).
 */

@Serializable
data class TarotCard(
    val id: Int,
    val name_zh: String,
    val name_en: String,
    val image_res: String,
    val arcana: String,           // "major" | "minor"
    val suit: String? = null,     // null for major
)

@Serializable
data class TarotDeck(
    val version: String,
    val source: String,
    val count: Int,
    val cards: List<TarotCard>,
)
