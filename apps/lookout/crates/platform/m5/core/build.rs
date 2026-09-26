use std::path::{Path, PathBuf};
use std::{env, fs};

const VERSION_FILE: &str = "crossings.version";
const ARTIFACTS: &str = "data/medallion/gold/artifact=crossings";
const PACKED: &str = "crossings.pointset";

fn main() {
    let app = app_root();
    let version_file = app.join(VERSION_FILE);
    let version = fs::read_to_string(&version_file)
        .unwrap_or_else(|err| panic!("read {}: {err}", version_file.display()))
        .trim()
        .to_string();

    let packed = app
        .join(ARTIFACTS)
        .join(format!("version={version}"))
        .join(PACKED);
    let bytes = fs::metadata(&packed)
        .unwrap_or_else(|err| panic!("read {}: {err}", packed.display()))
        .len();

    let declaration = format!(
        "/// The crossings `{version}` packed, as `{VERSION_FILE}` names them.\n\
         static PACKED: &Aligned<[u8; {bytes}]> = &Aligned(*include_bytes!({packed:?}));\n"
    );
    let out = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR")).join("carried.rs");
    fs::write(&out, declaration).unwrap_or_else(|err| panic!("write {}: {err}", out.display()));

    println!("cargo:rerun-if-changed={}", version_file.display());
    println!("cargo:rerun-if-changed={}", packed.display());
}

fn app_root() -> PathBuf {
    let manifest = env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    Path::new(&manifest)
        .ancestors()
        .nth(4)
        .expect("the crate sits four deep in the app")
        .to_path_buf()
}
