//! `beekeeper-plugin-pack <core.wasm> <out.wasm>` — wrap a core module built
//! with wit-bindgen for `wasm32-unknown-unknown` into a component, and print
//! its sha256 (the registry pin).
//!
//! Packing does not check what the component imports; the host does that at
//! load, and refuses anything outside `beekeeper:plugin@1`.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output] = args.as_slice() else {
        eprintln!("usage: beekeeper-plugin-pack <core.wasm> <component.wasm>");
        return ExitCode::from(2);
    };
    let core = match std::fs::read(input) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("cannot read {input}: {error}");
            return ExitCode::from(1);
        }
    };
    let component = match wit_component::ComponentEncoder::default()
        .module(&core)
        .and_then(|encoder| encoder.validate(true).encode())
    {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("cannot make a component of {input}: {error:#}");
            return ExitCode::from(1);
        }
    };
    if let Err(error) = std::fs::write(output, &component) {
        eprintln!("cannot write {output}: {error}");
        return ExitCode::from(1);
    }
    println!("{}", beekeeper_plugin_host::sha256_hex(&component));
    ExitCode::SUCCESS
}
