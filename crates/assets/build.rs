fn main() {
    // Keep in sync with the `fs_embed! { struct Assets, ... }` invocation in
    // `src/assets.rs`.
    fs_embed_build::generate(&fs_embed_build::FsEmbed {
        struct_name: "Assets",
        crate_path: "::util::__rust_embed",
        crate_relative: "../../assets",
        includes: &[
            "fonts/**/*",
            "icons/**/*",
            "images/**/*",
            "themes/**/*",
            "sounds/**/*",
            "prompts/**/*",
            "*.md",
        ],
        excludes: &["themes/src/*", "*.DS_Store"],
    });
}
