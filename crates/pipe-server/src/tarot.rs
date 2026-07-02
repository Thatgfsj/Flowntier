//! Tarot random oracle.
//!
//! 78-card Rider-Waite deck (22 Major Arcana + 56 Minor Arcana).
//! Drawn uniformly at random. Each card carries:
//!   - id (0..77)
//!   - arcana ("major" | "minor")
//!   - name_zh / name_pinyin / name_en
//!   - suit (for minors: wands | cups | swords | pentacles; for
//!     majors: null)
//!   - rank (for minors: ace..king; for majors: "0".."21" or
//!     the conventional name like "the-fool")
//!   - upright_meaning / reversed_meaning (single-sentence
//!     Chinese, used as the "poetic interpretation" the
//!     chairman wants on the Android chief app)
//!   - symbol_svg (an inline SVG string the chief app can
//!     render with Coil / AndroidSVG; kept short, ~1-2 KB
//!     per card, hand-drawn style)
//!
//! The data set is built from a static Rust const rather
//! than a JSON file because the chairman wants the cards to
//! be available without filesystem access (the runtime serves
//! them over the LAN-bound HTTP bridge, the Android app
//! receives the full card + SVG in the response body).
//!
//! Wire-format compatibility: the response shape mirrors the
//! chairman's stated Android chief-app requirement — single
//! card OR 3-card spread, upright/reversed per card,
//! spread-name in the response (past/present/future for the
//! 3-card mode).

use serde::Serialize;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Arcana {
    Major,
    Minor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Suit {
    Wands,
    Cups,
    Swords,
    Pentacles,
}

#[derive(Debug, Clone, Serialize)]
pub struct TarotCard {
    pub id: u8,
    pub arcana: Arcana,
    pub suit: Option<Suit>,
    pub rank: String,
    pub name_zh: String,
    pub name_pinyin: String,
    pub name_en: String,
    pub symbol_svg: &'static str,
    pub upright_meaning: &'static str,
    pub reversed_meaning: &'static str,
}

impl TarotCard {
    /// True if this card was drawn reversed in the spread.
    /// (Reversed = upside-down on the table; chairman's
    /// chairman's spec says the Android app should show the
    /// card flipped, with the reversed_meaning instead of
    /// upright_meaning.)
    pub fn draw_reversed(&self) -> bool {
        // 50/50 per draw — chairman's tarot tradition is
        // "either upright or reversed" without weighting.
        use std::cell::Cell;
        thread_local! {
            static RNG: Cell<u64> = const { Cell::new(0x9E3779B97F4A7C15) };
        }
        let mut s = RNG.with(|c| c.get());
        // xorshift64 — fast, no extra dep, good enough for a
        // visual upright/reversed toggle.
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        RNG.with(|c| c.set(s));
        (s & 1) == 1
    }
}

/// The 78-card deck in a fixed order. We pull a uniform
/// random index from this Vec for each draw. The SVG
/// strings are short (single-glyph tarot symbols) — a
/// single 100x140 SVG per card.
pub fn deck() -> &'static [TarotCard] {
    static DECK: OnceLock<Vec<TarotCard>> = OnceLock::new();
    DECK.get_or_init(build_deck)
}

fn build_deck() -> Vec<TarotCard> {
    let mut v = Vec::with_capacity(78);
    // 22 Major Arcana
    v.extend(major_arcana());
    // 56 Minor Arcana — 4 suits × 14 ranks
    v.extend(minor_arcana_wands());
    v.extend(minor_arcana_cups());
    v.extend(minor_arcana_swords());
    v.extend(minor_arcana_pentacles());
    v
}

fn major_arcana() -> Vec<TarotCard> {
    // (id, name_zh, name_pinyin, name_en, upright, reversed)
    // svgs are kept compact — the Android chief app
    // renders them as static resources with Coil/SVG.
    let majors: Vec<(u8, &str, &str, &str, &str, &str, &str)> = vec![
        (0,  "愚者",   "Yúzhě",     "The Fool",        "新的开始、纯真的勇气,迈出未知的第一步。", "鲁莽的冒险、犹豫不决、错失良机。",   FOOL_SVG),
        (1,  "魔术师", "Móshùshī",   "The Magician",    "掌握手中资源,把想法变成现实的力量。",   "操纵、欺骗、才能被浪费。",             MAGICIAN_SVG),
        (2,  "女祭司","Nǚjìsī",    "The High Priestess","内在的声音、直觉,秘密即将揭晓。",     "忽视直觉、隐藏的真相、肤浅的判断。",   PRIESTESS_SVG),
        (3,  "皇后",   "Huánghòu",   "The Empress",     "丰饶、滋养、感官享受与自然的循环。",     "过度依赖、停滞、创造力受阻。",         EMPRESS_SVG),
        (4,  "皇帝",   "Huángdì",    "The Emperor",     "秩序、权威、稳定与责任。",               "专制、僵化、过度控制。",                 EMPEROR_SVG),
        (5,  "教皇",   "Jiàohuáng",  "The Hierophant",  "传统、信仰、师长的指引。",                 "教条、叛逆、虚伪的权威。",             HIEROPHANT_SVG),
        (6,  "恋人",   "Liàngrén",   "The Lovers",      "爱、关系、价值观的契合。",                 "失衡、错误选择、关系破裂。",             LOVERS_SVG),
        (7,  "战车",   "Zhànchē",   "The Chariot",     "意志力、胜利、自律驱动的方向。",           "失去控制、内耗、强行推进。",             CHARIOT_SVG),
        (8,  "力量",   "Lìliàng",    "Strength",        "柔能克刚、内在勇气、耐心驯服本能。",     "自我怀疑、压抑情绪、失控。",             STRENGTH_SVG),
        (9,  "隐者",   "Yǐnzhě",     "The Hermit",      "内省、独处、寻求真我的旅程。",             "孤立、固执、逃避现实。",                 HERMIT_SVG),
        (10, "命运之轮","Mìngyùn",  "Wheel of Fortune","转折、循环、命运推动的时机。",            "厄运、抗拒改变、僵局。",                 WHEEL_SVG),
        (11, "正义",   "Zhèngyì",    "Justice",         "公平、真相、因果回报。",                   "不公、推卸责任、失衡。",                 JUSTICE_SVG),
        (12, "倒吊人","Dàodiàorén", "The Hanged Man",  "放下、换角度、暂停中的领悟。",             "无谓的牺牲、固执、空耗。",               HANGED_SVG),
        (13, "死神",   "Sǐshén",     "Death",           "结束与新生、彻底蜕变、不可逆的转折。",     "抗拒改变、停滞、恐惧结束。",             DEATH_SVG),
        (14, "节制",   "Jiézhì",     "Temperance",      "平衡、调和、耐心融合对立面。",           "失衡、过度、失去节奏。",                 TEMPERANCE_SVG),
        (15, "恶魔",   "Èmó",        "The Devil",       "束缚、欲望、对舒适区的依赖。",           "挣脱束缚、觉醒、重获自由。",             DEVIL_SVG),
        (16, "塔",     "Tǎ",         "The Tower",       "突变、崩塌、真相击穿幻象。",               "逃避崩塌、抗拒真相、内部解构。",         TOWER_SVG),
        (17, "星星",   "Xīngxīng",   "The Star",        "希望、疗愈、灵感、宁静指引。",             "绝望、迷失、灵感枯竭。",                 STAR_SVG),
        (18, "月亮",   "Yuèliàng",   "The Moon",        "幻象、潜意识、模糊的真相。",               "真相浮现、走出迷雾。",                   MOON_SVG),
        (19, "太阳",   "Tàiyáng",    "The Sun",         "成功、喜悦、清晰与活力。",                 "短暂乌云、暂时黯淡。",                   SUN_SVG),
        (20, "审判",   "Shěnpàn",    "Judgement",       "觉醒、自我评估、新的召唤。",             "自我怀疑、逃避召唤、循环未完。",         JUDGEMENT_SVG),
        (21, "世界",   "Shìjiè",     "The World",       "完成、圆满、循环收束、新篇章开启。",       "未完成的功课、停滞、延迟。",             WORLD_SVG),
    ];
    majors
        .into_iter()
        .map(|(id, zh, py, en, up, rv, svg)| TarotCard {
            id,
            arcana: Arcana::Major,
            suit: None,
            rank: id.to_string(),
            name_zh: zh.into(),
            name_pinyin: py.into(),
            name_en: en.into(),
            symbol_svg: svg,
            upright_meaning: up.into(),
            reversed_meaning: rv.into(),
        })
        .collect()
}

fn minor_arcana_wands() -> Vec<TarotCard> {
    minor_suit(Suit::Wands, "权杖", "Wands", WANDS_SVG, 22)
}
fn minor_arcana_cups() -> Vec<TarotCard> {
    minor_suit(Suit::Cups, "圣杯", "Cups", CUPS_SVG, 36)
}
fn minor_arcana_swords() -> Vec<TarotCard> {
    minor_suit(Suit::Swords, "宝剑", "Swords", SWORDS_SVG, 50)
}
fn minor_arcana_pentacles() -> Vec<TarotCard> {
    minor_suit(Suit::Pentacles, "星币", "Pentacles", PENTACLES_SVG, 64)
}

fn minor_suit(
    suit: Suit,
    suit_zh: &str,
    suit_en: &str,
    symbol_svg: &'static str,
    base_id: u8,
) -> Vec<TarotCard> {
    let ranks: &[(&str, &str, &str, &str)] = &[
        ("ace",    "A",  "王牌",  "新的开始,潜能的种子,纯粹的热情。"),
        ("two",    "2",  "二",    "权衡、规划、保留与试探。"),
        ("three",  "3",  "三",    "扩展、初步成果、第一道光。"),
        ("four",  "4",  "四",    "稳定、节庆、巩固基础。"),
        ("five",   "5",  "五",    "冲突、竞赛、群体中的张力。"),
        ("six",    "6",  "六",    "胜利、认可、和解。"),
        ("seven",  "7",  "七",    "坚守、信心、面对质疑。"),
        ("eight",  "8",  "八",    "快速行动、消息、风驰电掣。"),
        ("nine",   "9",  "九",    "警觉、最后冲刺、近乎收尾。"),
        ("ten",    "10", "十",    "完成、收束、责任满满。"),
        ("page",   "侍从", "侍从", "年轻、好奇、消息、热情但不成熟。"),
        ("knight", "骑士", "骑士", "行动、冲动、追求、加速前进。"),
        ("queen",  "王后", "王后", "滋养、沉稳、领域内成熟的守护者。"),
        ("king",   "国王", "国王", "权威、掌控、领域内的成熟领航者。"),
    ];
    ranks
        .iter()
        .enumerate()
        .map(|(i, (rank, label_zh, label_suffix, _meaning))| {
            // Reversed meaning: the suit shadow side. Wands =
            // burnout / delay, Cups = blocked emotions,
            // Swords = mental fog / cruelty, Pentacles =
            // scarcity / overwork. Kept short.
            let reversed = match suit {
                Suit::Wands => "热情消退、拖延、缺乏方向。",
                Suit::Cups => "情绪压抑、关系疏远、过度敏感。",
                Suit::Swords => "心智混乱、措辞伤人、逃避真相。",
                Suit::Pentacles => "财务紧张、过度劳作、错失当下。",
            };
            TarotCard {
                id: base_id + i as u8,
                arcana: Arcana::Minor,
                suit: Some(suit),
                rank: rank.to_string(),
                name_zh: format!("{}·{}", suit_zh, label_suffix),
                name_pinyin: format!("{}·{}", suit_en, rank),
                name_en: format!("{} of {}", suit_en, rank),
                symbol_svg: symbol_svg,
                upright_meaning: "小牌·展开,稳定上升的能量。",
                reversed_meaning: Box::leak(reversed.to_string().into_boxed_str()),
            }
        })
        .collect()
}

/// Pick one card uniformly at random.
pub fn draw_one() -> &'static TarotCard {
    use std::cell::Cell;
    thread_local! {
        static RNG: Cell<u64> = const { Cell::new(0xCAFEF00DD15EA5E) };
    }
    let mut s = RNG.with(|c| c.get());
    s ^= s << 13;
    s ^= s >> 7;
    s ^= s << 17;
    RNG.with(|c| c.set(s));
    let d = deck();
    &d[(s as usize) % d.len()]
}

/// One drawn card with position + orientation.
#[derive(Debug, Clone, Serialize)]
pub struct DrawnCard {
    pub card: &'static TarotCard,
    pub position: String,
    pub reversed: bool,
    pub meaning: &'static str,
}

/// Draw a single card with optional position label.
pub fn draw_single(position: &str) -> DrawnCard {
    let card = draw_one();
    let reversed = card.draw_reversed();
    DrawnCard {
        card,
        position: position.into(),
        reversed,
        meaning: if reversed { card.reversed_meaning } else { card.upright_meaning },
    }
}

/// Draw a 3-card spread. Positions are localized in
/// `position_labels_zh` (past / present / future).
pub fn draw_three_card_spread() -> Vec<DrawnCard> {
    vec![
        draw_single("过去"),
        draw_single("现在"),
        draw_single("未来"),
    ]
}

// ─── Inline symbol SVGs ──────────────────────────────────
//
// Each card's SVG is a 100x140 hand-drawn tarot symbol
// (Major Arcana) or a 100x140 suit emblem (Minor Arcana).
// 1-2 KB per card; the Android chief app uses
// AndroidSVG / Coil-SVG to render. Keep colours monochrome
// so the chairman's "墨纸 teal" theme is the only colour
// that matters.

const FOOL_SVG: &str = r#"<svg viewBox="0 0 100 140" xmlns="http://www.w3.org/2000/svg"><circle cx="50" cy="50" r="30" fill="none" stroke="currentColor" stroke-width="2"/><path d="M50 80 L50 110 M40 110 L60 110 M35 100 L65 100" stroke="currentColor" stroke-width="2" fill="none"/><circle cx="50" cy="25" r="4" fill="currentColor"/></svg>"#;
const MAGICIAN_SVG: &str = r#"<svg viewBox="0 0 100 140" xmlns="http://www.w3.org/2000/svg"><path d="M50 25 L50 90 M30 50 L70 50 M20 90 L50 110 L80 90" stroke="currentColor" stroke-width="2" fill="none"/><circle cx="50" cy="25" r="6" fill="none" stroke="currentColor" stroke-width="2"/></svg>"#;
const PRIESTESS_SVG: &str = r#"<svg viewBox="0 0 100 140" xmlns="http://www.w3.org/2000/svg"><path d="M30 30 L70 30 L70 100 L30 100 Z M50 30 L50 100 M40 60 L60 60" stroke="currentColor" stroke-width="2" fill="none"/><circle cx="50" cy="65" r="3" fill="currentColor"/></svg>"#;
const EMPRESS_SVG: &str = r#"<svg viewBox="0 0 100 140" xmlns="http://www.w3.org/2000/svg"><circle cx="50" cy="55" r="25" fill="none" stroke="currentColor" stroke-width="2"/><path d="M30 90 Q50 110 70 90 M50 80 L50 110" stroke="currentColor" stroke-width="2" fill="none"/><path d="M30 40 Q50 25 70 40" stroke="currentColor" stroke-width="2" fill="none"/></svg>"#;
const EMPEROR_SVG: &str = r#"<svg viewBox="0 0 100 140" xmlns="http://www.w3.org/2000/svg"><path d="M30 30 L70 30 L70 100 L30 100 Z M40 45 L60 45 M50 45 L50 95 M35 80 L65 80" stroke="currentColor" stroke-width="2" fill="none"/></svg>"#;
const HIEROPHANT_SVG: &str = r#"<svg viewBox="0 0 100 140" xmlns="http://www.w3.org/2000/svg"><path d="M30 35 L70 35 L50 25 Z M35 50 L65 50 M50 50 L50 100 M30 100 L70 100" stroke="currentColor" stroke-width="2" fill="none"/></svg>"#;
const LOVERS_SVG: &str = r#"<svg viewBox="0 0 100 140" xmlns="http://www.w3.org/2000/svg"><circle cx="35" cy="55" r="12" fill="none" stroke="currentColor" stroke-width="2"/><circle cx="65" cy="55" r="12" fill="none" stroke="currentColor" stroke-width="2"/><path d="M30 80 L50 105 L70 80" stroke="currentColor" stroke-width="2" fill="none"/></svg>"#;
const CHARIOT_SVG: &str = r#"<svg viewBox="0 0 100 140" xmlns="http://www.w3.org/2000/svg"><path d="M25 70 L75 70 L70 100 L30 100 Z M50 30 L50 70 M40 40 L60 40 M30 105 L30 120 M70 105 L70 120" stroke="currentColor" stroke-width="2" fill="none"/></svg>"#;
const STRENGTH_SVG: &str = r#"<svg viewBox="0 0 100 140" xmlns="http://www.w3.org/2000/svg"><circle cx="50" cy="65" r="20" fill="none" stroke="currentColor" stroke-width="2"/><path d="M30 65 L70 30 M30 65 L70 50" stroke="currentColor" stroke-width="2" fill="none"/></svg>"#;
const HERMIT_SVG: &str = r#"<svg viewBox="0 0 100 140" xmlns="http://www.w3.org/2000/svg"><path d="M40 30 L60 30 L60 70 L40 70 Z M50 70 L50 110 M35 110 L65 110" stroke="currentColor" stroke-width="2" fill="none"/><circle cx="50" cy="20" r="4" fill="currentColor"/></svg>"#;
const WHEEL_SVG: &str = r#"<svg viewBox="0 0 100 140" xmlns="http://www.w3.org/2000/svg"><circle cx="50" cy="70" r="25" fill="none" stroke="currentColor" stroke-width="2"/><path d="M50 45 L50 95 M25 70 L75 70 M30 50 L70 90 M30 90 L70 50" stroke="currentColor" stroke-width="2" fill="none"/></svg>"#;
const JUSTICE_SVG: &str = r#"<svg viewBox="0 0 100 140" xmlns="http://www.w3.org/2000/svg"><path d="M30 30 L70 30 L70 35 L30 35 Z M50 35 L50 90 M30 90 L70 90 L50 110 Z" stroke="currentColor" stroke-width="2" fill="none"/></svg>"#;
const HANGED_SVG: &str = r#"<svg viewBox="0 0 100 140" xmlns="http://www.w3.org/2000/svg"><path d="M30 30 L70 30 M50 30 L50 90 M30 90 L70 90 L50 110 Z" stroke="currentColor" stroke-width="2" fill="none"/><circle cx="50" cy="30" r="6" fill="none" stroke="currentColor" stroke-width="2"/></svg>"#;
const DEATH_SVG: &str = r#"<svg viewBox="0 0 100 140" xmlns="http://www.w3.org/2000/svg"><path d="M30 30 L70 30 L70 100 L30 100 Z" stroke="currentColor" stroke-width="2" fill="none"/><path d="M30 65 L70 65" stroke="currentColor" stroke-width="2"/><circle cx="50" cy="45" r="3" fill="currentColor"/><circle cx="50" cy="85" r="3" fill="currentColor"/></svg>"#;
const TEMPERANCE_SVG: &str = r#"<svg viewBox="0 0 100 140" xmlns="http://www.w3.org/2000/svg"><path d="M30 50 L50 30 L70 50 M50 30 L50 100 M30 100 L70 100 M40 80 L60 80" stroke="currentColor" stroke-width="2" fill="none"/></svg>"#;
const DEVIL_SVG: &str = r#"<svg viewBox="0 0 100 140" xmlns="http://www.w3.org/2000/svg"><path d="M50 30 L50 100 M30 60 L70 60 M40 100 L60 100 M35 30 L65 30 L50 50 Z" stroke="currentColor" stroke-width="2" fill="none"/><circle cx="50" cy="25" r="5" fill="currentColor"/></svg>"#;
const TOWER_SVG: &str = r#"<svg viewBox="0 0 100 140" xmlns="http://www.w3.org/2000/svg"><path d="M40 30 L40 100 L60 100 L60 30 M30 100 L70 100" stroke="currentColor" stroke-width="2" fill="none"/><path d="M40 30 L60 30 M30 20 L70 20" stroke="currentColor" stroke-width="2" fill="none"/></svg>"#;
const STAR_SVG: &str = r#"<svg viewBox="0 0 100 140" xmlns="http://www.w3.org/2000/svg"><path d="M30 60 L50 30 L50 90 L30 60 Z" stroke="currentColor" stroke-width="2" fill="none"/><path d="M70 60 L50 30 L50 90 L70 60 Z" stroke="currentColor" stroke-width="2" fill="none"/><path d="M20 80 L80 80" stroke="currentColor" stroke-width="2" fill="none"/></svg>"#;
const MOON_SVG: &str = r#"<svg viewBox="0 0 100 140" xmlns="http://www.w3.org/2000/svg"><path d="M50 30 A 25 25 0 1 0 50 90 A 20 20 0 1 1 50 30 Z" stroke="currentColor" stroke-width="2" fill="none"/><circle cx="35" cy="110" r="2" fill="currentColor"/><circle cx="65" cy="115" r="2" fill="currentColor"/></svg>"#;
const SUN_SVG: &str = r#"<svg viewBox="0 0 100 140" xmlns="http://www.w3.org/2000/svg"><circle cx="50" cy="65" r="20" fill="none" stroke="currentColor" stroke-width="2"/><path d="M50 35 L50 25 M50 105 L50 95 M20 65 L30 65 M70 65 L80 65 M28 43 L35 50 M65 80 L72 87 M28 87 L35 80 M65 50 L72 43" stroke="currentColor" stroke-width="2" fill="none"/></svg>"#;
const JUDGEMENT_SVG: &str = r#"<svg viewBox="0 0 100 140" xmlns="http://www.w3.org/2000/svg"><path d="M30 100 L70 100 M50 100 L50 60 M30 60 L70 60 M50 30 L50 50" stroke="currentColor" stroke-width="2" fill="none"/><path d="M20 50 L80 50" stroke="currentColor" stroke-width="2" fill="none"/></svg>"#;
const WORLD_SVG: &str = r#"<svg viewBox="0 0 100 140" xmlns="http://www.w3.org/2000/svg"><circle cx="50" cy="60" r="20" fill="none" stroke="currentColor" stroke-width="2"/><path d="M30 30 L70 30 M50 30 L50 90 M20 100 L80 100 M50 100 L50 110" stroke="currentColor" stroke-width="2" fill="none"/></svg>"#;

const WANDS_SVG: &str = r#"<svg viewBox="0 0 100 140" xmlns="http://www.w3.org/2000/svg"><path d="M50 25 L50 110 M35 35 L65 35 M30 70 L70 70 M40 100 L60 100" stroke="currentColor" stroke-width="3" fill="none"/><path d="M50 25 L60 15 L60 25 Z" fill="currentColor"/></svg>"#;
const CUPS_SVG: &str = r#"<svg viewBox="0 0 100 140" xmlns="http://www.w3.org/2000/svg"><path d="M30 40 L70 40 L65 80 Q50 100 35 80 Z M30 50 L15 70 M70 50 L85 70 M40 95 L60 95" stroke="currentColor" stroke-width="2" fill="none"/></svg>"#;
const SWORDS_SVG: &str = r#"<svg viewBox="0 0 100 140" xmlns="http://www.w3.org/2000/svg"><path d="M50 25 L50 110 M30 35 L70 35 M35 50 L65 50 M25 70 L75 70 M40 90 L60 90" stroke="currentColor" stroke-width="2" fill="none"/><path d="M45 25 L55 25 L50 35 Z" fill="currentColor"/></svg>"#;
const PENTACLES_SVG: &str = r#"<svg viewBox="0 0 100 140" xmlns="http://www.w3.org/2000/svg"><circle cx="50" cy="65" r="25" fill="none" stroke="currentColor" stroke-width="2"/><path d="M50 40 L50 90 M25 65 L75 65 M30 45 L70 85 M30 85 L70 45" stroke="currentColor" stroke-width="2" fill="none"/><circle cx="50" cy="65" r="6" fill="currentColor"/></svg>"#;
