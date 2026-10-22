//! Compiles the Slint UI (`ui/app.slint`) into Rust at build time.

fn main() {
    if let Err(error) = slint_build::compile("ui/app.slint") {
        panic!("the Slint UI failed to compile: {error}");
    }
}
