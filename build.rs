fn main() {
    // "native" resolves to the Qt style on Linux when the Qt backend is compiled
    // (backend-qt feature + Qt dev libraries present), otherwise falls back to fluent.
    let config = slint_build::CompilerConfiguration::new()
        // Embed the icon (SVG) and any other referenced assets directly into the
        // binary, so the app doesn't need external files next to it at run-time.
        .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles);
    slint_build::compile_with_config("ui/main.slint", config).unwrap();
}
