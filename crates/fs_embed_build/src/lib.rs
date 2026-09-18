//! Build-script support for [`util::fs_embed!`].
//!
//! The macro's compile-time arm reads a per-struct file list generated into
//! `OUT_DIR` by this crate instead of relying on `#[derive(RustEmbed)]`.
//! `rust-embed`'s derive snapshots the folder listing at proc-macro expansion
//! time and only re-scans on a full recompile, so a newly added asset (theme,
//! prompt, template, grammar, keymap) is silently omitted from incremental
//! builds. Generating the file list in a build script — and re-running that
//! script whenever a directory changes — makes new files show up without a
//! `cargo clean`.
//!
//! Each `fs_embed!` call site has a matching `build.rs` that calls [`generate`]
//! with the *same* `crate_relative` / `include` / `exclude` values as the macro
//! invocation. Keep them in sync; the macro only knows the struct name.

use std::path::{Path, PathBuf};

/// One `fs_embed!` call site to generate files for.
pub struct FsEmbed {
    /// The struct name from the `fs_embed!` invocation. The generated files are
    /// named `<struct_name>_fs_embed_get.rs` and `<struct_name>_fs_embed_iter.rs`
    /// so the macro can locate them via `stringify!`.
    pub struct_name: &'static str,
    /// The `crate_relative` value from the `fs_embed!` invocation, resolved
    /// against `CARGO_MANIFEST_DIR` of the crate declaring the call site.
    pub crate_relative: &'static str,
    /// The `include` globs from the `fs_embed!` invocation (empty = all files).
    pub includes: &'static [&'static str],
    /// The `exclude` globs from the `fs_embed!` invocation.
    pub excludes: &'static [&'static str],
}

/// Generate `<struct_name>_fs_embed_get.rs` and `<struct_name>_fs_embed_iter.rs`
/// into `OUT_DIR`, and tell cargo to re-run this script when any directory under
/// the embedded folder changes (so a new file or directory is picked up).
pub fn generate(fs_embed: &FsEmbed) {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let folder = manifest_dir.join(fs_embed.crate_relative);

    let matcher = rust_embed::utils::PathMatcher::new(fs_embed.includes, fs_embed.excludes);
    let files: Vec<rust_embed::utils::FileEntry> =
        rust_embed::utils::get_files(folder.to_string_lossy().into_owned(), matcher).collect();

    emit_rerun_if_changed(&folder);

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));

    // `get`: a `match` expression over `file_path`. The macro's surrounding
    // method brings `__FsEmbedFile` and `__FsEmbedMetadata` into scope so the
    // body compiles in any caller, including `util`'s own test module.
    let mut get = String::from("match file_path {\n");
    for file in &files {
        let rel = string_literal(&file.rel_path);
        let full = file.full_canonical_path.replace('\\', "/");
        get.push_str(&format!(
            "    {rel} => ::core::option::Option::Some(__FsEmbedFile {{\n        data: ::std::borrow::Cow::Borrowed(include_bytes!(\"{full}\")),\n        metadata: __FsEmbedMetadata::__rust_embed_new([0u8; 32], ::core::option::Option::None, ::core::option::Option::None),\n    }}),\n"
        ));
    }
    get.push_str("    _ => ::core::option::Option::None,\n}");
    write_generated(&out_dir, &format!("{}_fs_embed_get.rs", fs_embed.struct_name), &get);

    // `iter`: a static array of paths mapped to borrowed cows.
    let mut iter = String::from("[");
    for file in &files {
        iter.push_str(&string_literal(&file.rel_path));
        iter.push_str(", ");
    }
    iter.push_str("].into_iter().map(::std::borrow::Cow::Borrowed)");
    write_generated(&out_dir, &format!("{}_fs_embed_iter.rs", fs_embed.struct_name), &iter);
}

fn emit_rerun_if_changed(folder: &Path) {
    if !folder.is_dir() {
        return;
    }
    println!("cargo:rerun-if-changed={}", folder.display());
    if let Ok(entries) = std::fs::read_dir(folder) {
        for entry in entries.flatten() {
            emit_rerun_if_changed(&entry.path());
        }
    }
}

fn string_literal(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn write_generated(out_dir: &Path, name: &str, contents: &str) {
    let path = out_dir.join(name);
    if let Ok(existing) = std::fs::read_to_string(&path) {
        if existing == contents {
            return;
        }
    }
    std::fs::write(&path, contents)
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", path.display()));
}
