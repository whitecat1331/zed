fn main() {
    // Test-only call site in `src/util.rs` (backing `test_fs_embed_iter_and_get`
    // and the `debug-embed` arm). Keep in sync with that `fs_embed!` invocation.
    fs_embed_build::generate(&fs_embed_build::FsEmbed {
        struct_name: "FsEmbedTestAssets",
        crate_relative: "src",
        includes: &["*.rs"],
        excludes: &["test/**/*"],
    });
}
