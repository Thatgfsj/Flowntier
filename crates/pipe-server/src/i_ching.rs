//! I Ching (易经) random oracle.
//!
//! Loads the 64-hexagram dataset from a JSON file baked into
//! the binary at compile time (see `include_str!`). On each
//! `draw_hexagram()` call we pick one of the 64 hexagrams
//! uniformly at random using the thread-local XorWow RNG
//! from the `rand` crate.
//!
//! Wire-format compatibility: the response shape mirrors the
//! `crates/agent-core/src/prompt/role.json` shape that the
//! Tauri shell's `IChingOracle.tsx` component already
//! understands, so no UI changes are needed.

use serde::Deserialize;
use std::sync::OnceLock;

/// One I Ching hexagram.
#[derive(Debug, Clone, Deserialize)]
pub struct Hexagram {
    pub id: u8,
    pub name_zh: String,
    pub name_pinyin: String,
    pub name_en: String,
    /// King Wen bottom-up binary encoding: bit 0 (LSB) is the
    /// lowest line (line 1), bit 5 (MSB) is the top line
    /// (line 6). The six derived [Line] values are computed
    /// lazily at first read.
    pub binary: u8,
    pub judgment: String,
    pub image: String,
}

/// One line of a hexagram (yin or yang).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Yang,
    Yin,
}

#[derive(Debug, Clone, Copy)]
pub struct Line {
    pub position: u8,
    pub kind: LineKind,
    pub glyph: &'static str,
}

impl Hexagram {
    /// Six lines, ordered bottom (position 1) to top (position 6).
    pub fn lines(&self) -> [Line; 6] {
        let mut out = [Line {
            position: 1,
            kind: LineKind::Yang,
            glyph: "━━━━━━━━━━",
        }; 6];
        for (i, slot) in out.iter_mut().enumerate() {
            let yang = (self.binary >> i) & 1 == 1;
            slot.position = (i as u8) + 1;
            slot.kind = if yang { LineKind::Yang } else { LineKind::Yin };
            slot.glyph = if yang {
                "━━━━━━━━━━"
            } else {
                "━━    ━━"
            };
        }
        out
    }
}

#[derive(Debug, Deserialize)]
struct HexagramDataset {
    #[allow(dead_code)]
    schema_version: u32,
    #[allow(dead_code)]
    source: String,
    #[allow(dead_code)]
    note: String,
    hexagrams: Vec<Hexagram>,
}

static CACHE: OnceLock<Vec<Hexagram>> = OnceLock::new();

fn cache() -> &'static Vec<Hexagram> {
    CACHE.get_or_init(|| {
        let raw = include_str!("hexagrams.json");
        let parsed: HexagramDataset = serde_json::from_str(raw)
            .expect("hexagrams.json is well-formed; this is a build error if it fails");
        assert_eq!(
            parsed.hexagrams.len(),
            64,
            "hexagrams.json must contain exactly 64 entries"
        );
        // Validate canonical ordering: ids 1..=64.
        for (i, h) in parsed.hexagrams.iter().enumerate() {
            assert_eq!(
                h.id as usize,
                i + 1,
                "hexagrams.json must use canonical 1..=64 ids"
            );
        }
        parsed.hexagrams
    })
}

/// All 64 hexagrams in canonical King Wen order.
pub fn all_hexagrams() -> Result<Vec<Hexagram>, String> {
    Ok(cache().clone())
}

/// Draw one hexagram uniformly at random.
pub fn draw_hexagram() -> Result<Hexagram, String> {
    let hexes = cache();
    if hexes.is_empty() {
        return Err("hexagram dataset is empty".into());
    }
    // rand::random() is uniform on [0, 2^64); take mod len
    // for index. len is 64, no modulo bias at this size.
    let idx = rand::random::<u64>() as usize % hexes.len();
    Ok(hexes[idx].clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dataset_has_64_hexagrams() {
        assert_eq!(cache().len(), 64);
    }

    #[test]
    fn ids_are_1_through_64() {
        let hexes = cache();
        for (i, h) in hexes.iter().enumerate() {
            assert_eq!(
                h.id as usize,
                i + 1,
                "id at index {} should be {}",
                i,
                i + 1
            );
        }
    }

    #[test]
    fn first_hexagram_is_qian_all_yang() {
        let qian = &cache()[0];
        assert_eq!(qian.id, 1);
        assert_eq!(qian.binary, 0b111111);
        let lines = qian.lines();
        for line in &lines {
            assert_eq!(line.kind, LineKind::Yang);
            assert_eq!(line.glyph, "━━━━━━━━━━");
        }
    }

    #[test]
    fn second_hexagram_is_kun_all_yin() {
        let kun = &cache()[1];
        assert_eq!(kun.id, 2);
        assert_eq!(kun.binary, 0b000000);
        let lines = kun.lines();
        for line in &lines {
            assert_eq!(line.kind, LineKind::Yin);
            assert_eq!(line.glyph, "━━    ━━");
        }
    }

    #[test]
    fn draw_returns_valid_hexagram() {
        for _ in 0..20 {
            let h = draw_hexagram().expect("draw");
            assert!((1..=64).contains(&h.id));
            assert_eq!(h.lines().len(), 6);
        }
    }
}
