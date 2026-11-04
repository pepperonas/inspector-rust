//! Every built-in shader effect (`core/rust-lib/assets/record/effects/`) goes through the
//! compositor's own validation and Metal compile, exactly as the editor's scene hands it over.
//! The parameters come from the one declaration core's registry reads (`params.json`), so a
//! WGSL file and its declaration cannot drift apart. This crate cannot link core: the files are
//! read by path.
//!
//! ```sh
//! MAC_FFMPEG_DIR=third_party/ffmpeg/8.1.2 cargo test -p record-render --test effects_compile
//! ```
#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use openscreen_compositor::scene::ShaderParamDecl;
use openscreen_compositor::shader_fx::{check_for_agent, Tier};
use serde_json::Value;

const PARAMS_JSON: &str = include_str!("../../core/rust-lib/assets/record/effects/params.json");

const CODE: &[(&str, &str)] = &[
    ("vignette", include_str!("../../core/rust-lib/assets/record/effects/vignette.wgsl")),
    ("grain", include_str!("../../core/rust-lib/assets/record/effects/grain.wgsl")),
    ("chromatic", include_str!("../../core/rust-lib/assets/record/effects/chromatic.wgsl")),
    ("ripple", include_str!("../../core/rust-lib/assets/record/effects/ripple.wgsl")),
    ("spotlight", include_str!("../../core/rust-lib/assets/record/effects/spotlight.wgsl")),
    ("scanlines", include_str!("../../core/rust-lib/assets/record/effects/scanlines.wgsl")),
];

/// `(id, the declared {name, type} pairs)` from `params.json`, in its order.
fn declared() -> Vec<(String, Vec<ShaderParamDecl>)> {
    let v: Vec<Value> = serde_json::from_str(PARAMS_JSON).expect("params.json");
    v.into_iter()
        .map(|e| {
            let id = e["id"].as_str().expect("id").to_string();
            // The compositor's decl reads `name` + `type` and ignores min/max/default.
            let params: Vec<ShaderParamDecl> = serde_json::from_value(e["params"].clone()).expect("params");
            (id, params)
        })
        .collect()
}

#[test]
fn every_builtin_effect_compiles() {
    let decls = declared();
    assert_eq!(
        decls.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(),
        CODE.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        "every declared effect has its WGSL file, in the same order"
    );
    for ((id, params), (_, code)) in decls.iter().zip(CODE) {
        let report = check_for_agent(code, params).unwrap_or_else(|e| panic!("{id}: {e}"));
        // Built-ins stay cheap: at most a handful of frame reads per pixel.
        assert!(report.tier <= Tier::Medium, "{id}: {report:?}");
        eprintln!("{id}: {report:?}");
    }
}
