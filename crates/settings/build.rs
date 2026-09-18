fn main() {
    // Keep in sync with the `fs_embed! { struct SettingsAssets, ... }`
    // invocation in `src/settings.rs`.
    fs_embed_build::generate(&fs_embed_build::FsEmbed {
        struct_name: "SettingsAssets",
        crate_relative: "../../assets",
        includes: &["settings/*", "keymaps/*"],
        excludes: &["*.DS_Store"],
    });
}
