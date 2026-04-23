// This code is part of Qiskit.
//
// (C) Copyright IBM 2025
//
// This code is licensed under the Apache License, Version 2.0. You may
// obtain a copy of this license in the LICENSE.txt file in the root directory
// of this source tree or at http://www.apache.org/licenses/LICENSE-2.0.
//
// Any modifications or derivative works of this code must retain this
// copyright notice, and modified files need to carry a notice indicating
// that they have been altered from the originals.

// For C API bindings
fn main() {
    // Build the qasm3-to-ionq-qis C++ translator via cmake.
    let dst = cmake::Config::new("dependencies/qasm3_to_ionq_qis")
        .build_target("qasm3_to_ionq_qis_core")
        .build();

    println!(
        "cargo:rustc-link-search=native={}/build",
        dst.display()
    );
    println!("cargo:rustc-link-lib=static=qasm3_to_ionq_qis_core");

    // Link the ANTLR4 runtime (built by cmake FetchContent into the build tree).
    // The exact path depends on the cmake build layout.
    let antlr_search = format!(
        "{}/build/_deps/antlr4_runtime-build/runtime",
        dst.display()
    );
    println!("cargo:rustc-link-search=native={antlr_search}");
    println!("cargo:rustc-link-lib=static=antlr4-runtime");

    // Link the C++ standard library.
    if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-lib=c++");
    } else {
        println!("cargo:rustc-link-lib=stdc++");
    }

    println!("cargo:rerun-if-changed=dependencies/qasm3_to_ionq_qis/");

    for (key, value) in std::env::vars() {
        eprintln!("{key}: {value}");
    }
    // Pull the config from the cbindgen.toml file.
    let config = cbindgen::Config::from_file("cbindgen.toml").unwrap();

    match cbindgen::generate_with_config(".", config) {
        Ok(value) => {
            value.write_to_file("qrmi.h");
        }
        Err(e) => {
            eprintln!("{}", e);
        }
    }

    println!("cargo:rerun-if-changed=/src/*");
    println!("cargo:rerun-if-changed=/build.rs");
}
