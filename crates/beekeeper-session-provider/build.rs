//! Link-time settings for the provider binary.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // Windows MSVC gives the main thread 1 MiB; macOS and Linux give it 8 MiB.
    // The provider's `#[tokio::main]` runs `run()` on that thread, and in a debug
    // build it does not fit in 1 MiB: the Host's supervised provider exited
    // 0xC00000FD (stack overflow) on every start and the Host backed off forever.
    // Ask the linker for the 8 MiB the other platforms already have. Nothing
    // changes on Unix.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/STACK:8388608");
    }
}
