fn main() {
    // "native" resolves to the Qt style on Linux when the Qt backend is compiled
    // (backend-qt feature + Qt dev libraries present), otherwise falls back to fluent.
    let config = slint_build::CompilerConfiguration::new().with_style("native".into());
    slint_build::compile_with_config("ui/main.slint", config).unwrap();
}
