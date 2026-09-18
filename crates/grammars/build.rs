fn main() {
    // Keep in sync with the `fs_embed! { struct GrammarDir, ... }` invocation in
    // `src/grammars.rs`.
    fs_embed_build::generate(&fs_embed_build::FsEmbed {
        struct_name: "GrammarDir",
        crate_relative: "src/",
        includes: &[],
        excludes: &["*.rs"],
    });
}
