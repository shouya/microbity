use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

fn linker_data() -> Option<&'static [u8]> {
  #[cfg(feature = "softdevice")]
  return Some(include_bytes!("memory-nrf52833-with-softdevice.x"));

  #[cfg(not(feature = "softdevice"))]
  return None;
}

fn main() {
  // Put `memory.x` in our output directory and ensure it's
  // on the linker search path.
  let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap());
  if let Some(data) = linker_data() {
    File::create(out.join("memory.x"))
      .unwrap()
      .write_all(data)
      .unwrap();
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed=memory.x");
  }

  // By default, Cargo will re-run a build script whenever
  // any file in the project changes. By specifying `memory.x`
  // here, we ensure the build script is only re-run when
  // `memory.x` is changed.
  println!("cargo:rustc-link-arg-bins=--nmagic");
  // println!("cargo:rustc-link-arg-bins=-Tlink.x");
  // println!("cargo:rustc-link-arg-bins=-Tdefmt.x");
}
