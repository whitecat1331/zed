//! Build-script support for [`util::fs_embed!`].
//!
//! The macro's compile-time arm includes a per-struct `impl` block generated
//! into `OUT_DIR` by this crate instead of relying on `#[derive(RustEmbed)]`.
//! `rust-embed`'s derive snapshots the folder listing at proc-macro expansion
//! time and only re-scans on a full recompile, so a newly added asset (theme,
//! prompt, template, grammar, keymap) is silently omitted from incremental
//! builds. Generating the file list in a build script — and re-running that
//! script whenever a directory changes — makes new files show up without a
//! `cargo clean`.
//!
//! Each `fs_embed!` call site has a matching `build.rs` that calls [`generate`]
//! with the *same* `crate_relative` / `include` / `exclude` values as the macro
//! invocation, plus the struct name and the path to `util`'s `__rust_embed`
//! re-export. Keep them in sync; the macro only knows the struct name.

use std::path::{Path, PathBuf};

/// One `fs_embed!` call site to generate an impl block for.
pub struct FsEmbed {
    /// The struct name from the `fs_embed!` invocation. The generated file is
    /// named `<struct_name>_fs_embed_impl.rs` so the macro can locate it via
    /// `stringify!`.
    pub struct_name: &'static str,
    /// Path to `util`'s `__rust_embed` re-export, as seen from the crate being
    /// compiled: `::util::__rust_embed` for callers, `crate::__rust_embed` for
    /// `util`'s own test call site.
    pub crate_path: &'static str,
    /// The `crate_relative` value from the `fs_embed!` invocation, resolved
    /// against `CARGO_MANIFEST_DIR` of the crate declaring the call site.
    pub crate_relative: &'static str,
    /// The `include` globs from the `fs_embed!` invocation (empty = all files).
    pub includes: &'static [&'static str],
    /// The `exclude` globs from the `fs_embed!` invocation.
    pub excludes: &'static [&'static str],
}

/// Generate `<struct_name>_fs_embed_impl.rs` into `OUT_DIR`, and tell cargo to
/// re-run this script when any directory under the embedded folder changes (so a
/// new file or directory is picked up).
pub fn generate(fs_embed: &FsEmbed) {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let folder = manifest_dir.join(fs_embed.crate_relative);

    let matcher = rust_embed::utils::PathMatcher::new(fs_embed.includes, fs_embed.excludes);
    let files: Vec<rust_embed::utils::FileEntry> =
        rust_embed::utils::get_files(folder.to_string_lossy().into_owned(), matcher).collect();

    emit_rerun_if_changed(&folder);

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));

    let name = fs_embed.struct_name;
    let crate_path = fs_embed.crate_path;

    // The generated file is self-contained so `include!` never has to resolve an
    // identifier through macro hygiene: it declares its own `file_path`
    // parameter and refers to the embedded-file types by full path.
    let mut body = String::new();
    body.push_str(&format!("impl {name} {{\n"));
    body.push_str(&format!(
        "    pub fn get(file_path: &str) -> ::core::option::Option<{crate_path}::EmbeddedFile> {{\n"
    ));
    body.push_str("        match file_path {\n");
    for file in &files {
        let rel = string_literal(&file.rel_path);
        let full = file.full_canonical_path.replace('\\', "/");
        body.push_str(&format!(
            "            {rel} => ::core::option::Option::Some({crate_path}::EmbeddedFile {{\n"
        ));
        body.push_str(&format!(
            "                data: ::std::borrow::Cow::Borrowed(include_bytes!(\"{full}\")),\n"
        ));
        body.push_str(&format!(
            "                metadata: {crate_path}::Metadata::__rust_embed_new([0u8; 32], ::core::option::Option::None, ::core::option::Option::None),\n"
        ));
        body.push_str("            }),\n");
    }
    body.push_str("            _ => ::core::option::Option::None,\n");
    body.push_str("        }\n");
    body.push_str("    }\n\n");
    body.push_str(
        "    pub fn iter() -> impl ::core::iter::Iterator<Item = ::std::borrow::Cow<'static, str>> + 'static {\n",
    );
    body.push_str("        [");
    for file in &files {
        body.push_str(&string_literal(&file.rel_path));
        body.push_str(", ");
    }
    body.push_str("].into_iter().map(::std::borrow::Cow::Borrowed)\n");
    body.push_str("    }\n");
    body.push_str("}\n\n");

    body.push_str(&format!("impl {crate_path}::RustEmbed for {name} {{\n"));
    body.push_str(&format!(
        "    fn get(file_path: &str) -> ::core::option::Option<{crate_path}::EmbeddedFile> {{\n"
    ));
    body.push_str(&format!("        {name}::get(file_path)\n"));
    body.push_str("    }\n\n");
    body.push_str(
        "    fn iter() -> impl ::core::iter::Iterator<Item = ::std::borrow::Cow<'static, str>> + 'static {\n",
    );
    body.push_str(&format!("        {name}::iter()\n"));
    body.push_str("    }\n");
    body.push_str("}\n");

    write_generated(&out_dir, &format!("{name}_fs_embed_impl.rs"), &body);
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
