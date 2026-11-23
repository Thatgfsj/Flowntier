//! Plugin registry + I Ching routes.

use super::*;

pub(super) fn register_plugins(d: &mut Dispatcher, _state: Arc<ServerState>) {
    // Plugin registry. v0.3 ships zero user-loadable plugins in
    // the sidecar; the Tauri shell's plugin panel will render
    // an empty list, which matches the spec.
    d.register("GET", "/api/plugins", |_body| {
        Box::pin(async {
            Ok((
                200,
                json!({
                    "plugins": [],
                    "note": "no plugins registered in v0.3; the Tauri shell's plugin panel is empty by design.",
                }),
            ))
        })
    });
    d.register("POST", "/api/plugins/{name}/invoke", |body| {
        let _ = body;
        Box::pin(async {
            Ok((
                501,
                json!({
                    "error": "plugin invocation not yet implemented; no plugins are registered in v0.3.",
                }),
            ))
        })
    });
}

pub(super) fn register_i_ching(d: &mut Dispatcher, _state: Arc<ServerState>) {
    // I Ching oracle (64-gua divination). Implements the full
    // King Wen sequence. The data set is a 12 KB JSON file
    // baked into the binary; the draw path uses
    // `rand::random::<u64>()` for uniform selection across the
    // 64 hexagrams.
    d.register("GET", "/api/i_ching/draw", |_body| {
        Box::pin(async {
            match crate::i_ching::draw_hexagram() {
                Ok(hex) => Ok((
                    200,
                    json!({
                        "draw": {
                            "id": hex.id,
                            "name_zh": hex.name_zh,
                            "name_pinyin": hex.name_pinyin,
                            "name_en": hex.name_en,
                            "binary": hex.binary,
                            "lines": hex.lines().iter().map(|l| json!({
                                "position": l.position,
                                "kind": match l.kind {
                                    crate::i_ching::LineKind::Yang => "yang",
                                    crate::i_ching::LineKind::Yin  => "yin",
                                },
                                "glyph": l.glyph,
                            })).collect::<Vec<_>>(),
                            "judgment": hex.judgment,
                            "image": hex.image,
                        },
                        "drawn_at_ms": std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0),
                    }),
                )),
                Err(e) => Ok((500, json!({ "error": e }))),
            }
        })
    });
    d.register("GET", "/api/i_ching/list", |_body| {
        Box::pin(async {
            match crate::i_ching::all_hexagrams() {
                Ok(hexes) => Ok((
                    200,
                    json!({
                        "list": hexes.iter().map(|h| json!({
                            "id": h.id,
                            "name_zh": h.name_zh,
                            "name_pinyin": h.name_pinyin,
                            "name_en": h.name_en,
                            "binary": h.binary,
                        })).collect::<Vec<_>>(),
                        "count": hexes.len(),
                    }),
                )),
                Err(e) => Ok((500, json!({ "error": e }))),
            }
        })
    });
}
