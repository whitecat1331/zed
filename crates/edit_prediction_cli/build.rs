fn main() {
    let cargo_toml =
        std::fs::read_to_string("../zed/Cargo.toml").expect("Failed to read crates/zed/Cargo.toml");
    let version = cargo_toml
        .lines()
        .find(|line| line.starts_with("version = "))
        .expect("Version not found in crates/zed/Cargo.toml")
        .split('=')
        .nth(1)
        .expect("Invalid version format")
        .trim()
        .trim_matches('"');
    println!("cargo:rustc-env=ZED_PKG_VERSION={}", version);

    // Keep in sync with the `fs_embed!` invocations in `src/prompt_assets.rs`
    // and `src/filter_languages.rs`.
    fs_embed_build::generate(&fs_embed_build::FsEmbed {
        struct_name: "EmbeddedPrompts",
        crate_relative: "src/prompts",
        includes: &[],
        excludes: &[],
    });
    fs_embed_build::generate(&fs_embed_build::FsEmbed {
        struct_name: "LanguageConfigs",
        crate_relative: "../grammars/src/",
        includes: &["*/config.toml"],
        excludes: &[],
    });
}
