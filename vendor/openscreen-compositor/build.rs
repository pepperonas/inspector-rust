//! Build script of the vendored OpenScreen compositor (upstream: crates/compositor/build.rs,
//! OpenScreen v2.1.2 bfd5bc32). Reduced to the macOS path Inspector Rust uses; see VENDORED.md.
//!
//! What it does (macOS target on Apple Silicon only):
//!   1. Points `LIBCLANG_PATH` at the active Xcode/CLT toolchain for bindgen.
//!   2. Finds an LGPL ffmpeg 8.1.2 tree (`include/` + `lib/`): `MAC_FFMPEG_DIR`, else the
//!      repo's `third_party/ffmpeg/8.1.2` built by `scripts/build-ffmpeg-macos.sh`,
//!      else `FFMPEG_DIR`.
//!   3. Links the shared libs avformat/avcodec/avutil/swscale/swresample/avfilter.
//!   4. Compiles `shim.c` (accessors for opaque AVFormatContext fields + AVERROR(EAGAIN)/EOF).
//!   5. Runs bindgen over `wrapper_macos.h` into `$OUT_DIR/ffi.rs`.
//!   6. Emits `cfg(openscreen_ffmpeg)`, which turns the library on (src/lib.rs).
//!
//! On any other target, or without an ffmpeg tree, nothing is built and the crate is an
//! empty library (a `cargo:warning` says why on macOS/arm64).

fn main() {
    println!("cargo::rustc-check-cfg=cfg(openscreen_ffmpeg)");
    #[cfg(target_os = "macos")]
    macos_host::run();
}

#[cfg(target_os = "macos")]
mod macos_host {
    use std::env;
    use std::path::{Path, PathBuf};

    const FFMPEG_VERSION: &str = "8.1.2";

    pub fn run() {
        let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
        let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
        println!("cargo:rerun-if-env-changed=MAC_FFMPEG_DIR");
        println!("cargo:rerun-if-env-changed=FFMPEG_DIR");
        println!("cargo:rerun-if-changed=wrapper_macos.h");
        println!("cargo:rerun-if-changed=shim.c");
        if target_os != "macos" || target_arch != "aarch64" {
            return;
        }

        let repo_tree = repo_ffmpeg_tree();
        if let Some(t) = &repo_tree {
            // Re-run once the script has built (or rebuilt) the tree.
            println!("cargo:rerun-if-changed={}", t.join(".ir-ffmpeg-stamp").display());
        }
        let ff = env::var("MAC_FFMPEG_DIR")
            .ok()
            .map(PathBuf::from)
            .filter(|p| usable_ffmpeg_tree(p))
            .or_else(|| repo_tree.filter(|p| usable_ffmpeg_tree(p)))
            .or_else(|| {
                env::var("FFMPEG_DIR")
                    .ok()
                    .map(PathBuf::from)
                    .filter(|p| usable_ffmpeg_tree(p))
            });
        let Some(ff) = ff else {
            println!(
                "cargo:warning=openscreen-compositor: no ffmpeg {FFMPEG_VERSION} tree found \
                 (MAC_FFMPEG_DIR / third_party/ffmpeg/{FFMPEG_VERSION}); building an empty \
                 library. Run scripts/build-ffmpeg-macos.sh."
            );
            return;
        };

        point_libclang_at_the_xcode_toolchain();

        let include_dir = ff.join("include");
        let lib_dir = ff.join("lib");
        println!("cargo:rustc-link-search=native={}", lib_dir.display());
        for lib in ["avformat", "avcodec", "avutil", "swscale", "swresample", "avfilter"] {
            println!("cargo:rustc-link-lib=dylib={lib}");
        }

        cc::Build::new()
            .file("shim.c")
            .include(&include_dir)
            .compile("sn_shim");

        // The clang target follows the cargo TARGET arch (upstream hard-coded
        // aarch64-apple-darwin; we only reach this on aarch64 today, but the layout of the
        // generated structs must never silently disagree with the target).
        let mut builder = bindgen::Builder::default()
            .header("wrapper_macos.h")
            .clang_arg(format!("-I{}", include_dir.display()))
            .clang_arg(format!("--target={target_arch}-apple-darwin"))
            .allowlist_function("av.*")
            .allowlist_function("avcodec_.*")
            .allowlist_function("avformat_.*")
            .allowlist_function("avio_.*")
            .allowlist_function("swr_.*")
            .allowlist_function("sws_.*")
            .allowlist_type("AV.*")
            .allowlist_type("SwrContext")
            .allowlist_type("SwsContext")
            .allowlist_var("SWS_.*")
            .allowlist_var("AV_.*")
            .allowlist_var("AVERROR.*")
            .allowlist_var("FF_.*")
            .allowlist_var("AVIO_.*")
            .default_enum_style(bindgen::EnumVariation::ModuleConsts)
            .derive_default(true)
            .layout_tests(false);
        if let Ok(sysroot) = std::process::Command::new("xcrun")
            .args(["--show-sdk-path", "--sdk", "macosx"])
            .output()
        {
            if let Ok(s) = std::str::from_utf8(&sysroot.stdout) {
                let s = s.trim();
                if !s.is_empty() {
                    builder = builder.clang_arg("-isysroot").clang_arg(s);
                }
            }
        }
        let bindings = builder
            .generate()
            .expect("bindgen failed on the ffmpeg headers");
        let out = PathBuf::from(env::var("OUT_DIR").unwrap());
        bindings
            .write_to_file(out.join("ffi.rs"))
            .expect("writing ffi.rs");

        println!("cargo:rustc-cfg=openscreen_ffmpeg");
        // Inspector Rust: tell dependents (the `ir-record-render` helper) that the library is
        // real, and where its dylibs are (DEP_OPENSCREEN_FFMPEG_ENABLED / _LIB_DIR).
        println!("cargo:enabled=1");
        println!("cargo:lib_dir={}", lib_dir.display());
    }

    /// `<repo>/third_party/ffmpeg/8.1.2` (vendor/openscreen-compositor → ../../).
    fn repo_ffmpeg_tree() -> Option<PathBuf> {
        let manifest = env::var("CARGO_MANIFEST_DIR").ok()?;
        Some(
            Path::new(&manifest)
                .parent()?
                .parent()?
                .join("third_party")
                .join("ffmpeg")
                .join(FFMPEG_VERSION),
        )
    }

    /// A usable tree has both `include/` (bindgen) and `lib/` (linking).
    fn usable_ffmpeg_tree(dir: &Path) -> bool {
        dir.join("include").is_dir() && dir.join("lib").is_dir()
    }

    /// Sets `LIBCLANG_PATH` to the active Xcode/CommandLineTools toolchain unless a valid
    /// one is already set (upstream behaviour, kept verbatim in substance).
    fn point_libclang_at_the_xcode_toolchain() {
        println!("cargo:rerun-if-env-changed=LIBCLANG_PATH");
        if let Ok(v) = env::var("LIBCLANG_PATH") {
            if Path::new(&v).join("libclang.dylib").exists() {
                return;
            }
        }
        let lib_dir = std::process::Command::new("xcrun")
            .args(["--find", "clang"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .and_then(|clang| {
                Path::new(&clang)
                    .parent()?
                    .parent()
                    .map(|usr| usr.join("lib"))
            })
            .filter(|d| d.join("libclang.dylib").exists());
        match lib_dir {
            Some(d) => env::set_var("LIBCLANG_PATH", d),
            None => env::remove_var("LIBCLANG_PATH"),
        }
    }
}
