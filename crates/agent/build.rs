fn main() {
    // Keep in sync with the `fs_embed! { struct Assets, ... }` invocation in
    // `src/templates.rs`.
    fs_embed_build::generate(&fs_embed_build::FsEmbed {
        struct_name: "Assets",
        crate_relative: "src/templates",
        includes: &["*.hbs"],
        excludes: &[],
    });
}
