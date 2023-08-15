use std::env;
use std::path::PathBuf;

use bindgen::callbacks::{IntKind, ParseCallbacks};

#[derive(Debug)]
struct DefineIntModifier;

impl ParseCallbacks for DefineIntModifier {
    fn int_macro(&self, _name: &str, _value: i64) -> Option<IntKind> {
        if _name.starts_with("VA_STATUS_") && _name != "VA_STATUS_ERROR_UNKNOWN" {
            return Some(IntKind::I32);
        }
        None
    }
}

fn main() {
    println!("cargo:rerun-if-changed=bindgen_wrapper.h");

    // The bindgen::Builder is the main entry point
    // to bindgen, and lets you build up options for
    // the resulting bindings.
    let bindings = bindgen::Builder::default()
        // The input header we would like to generate
        // bindings for.
        .header("bindgen_wrapper.h")
        // Tell cargo to invalidate the built crate whenever any of the
        // included header files changed.
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        .derive_default(true)
        .parse_callbacks(Box::new(DefineIntModifier))
        // Finish the builder and generate the bindings.
        .generate()
        // Unwrap the Result and panic on failure.
        .expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/bindings.rs file.
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
