use std::path::PathBuf;
use std::{env, fs};

const SPEC: &str = "openapi.json";
const DECLARED_VERSION: &str = "3.0.3";

fn main() {
    println!("cargo:rerun-if-changed={SPEC}");

    let text = fs::read_to_string(SPEC).unwrap_or_else(|err| panic!("read {SPEC}: {err}"));
    let mut document: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|err| panic!("parse {SPEC}: {err}"));
    // progenitor reads OpenAPI 3.0 alone, and the 3.1 spec Motis publishes generates as 3.0.
    document["openapi"] = DECLARED_VERSION.into();
    let spec: openapiv3::OpenAPI = serde_json::from_value(document)
        .unwrap_or_else(|err| panic!("read {SPEC} as OpenAPI: {err}"));

    let mut settings = progenitor::GenerationSettings::default();
    settings.with_interface(progenitor::InterfaceStyle::Builder);
    let tokens = progenitor::Generator::new(&settings)
        .generate_tokens(&spec)
        .unwrap_or_else(|err| panic!("generate a client from {SPEC}: {err}"));
    let file = syn::parse2(tokens).expect("progenitor generates a parseable file");

    let out = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR")).join("api.rs");
    fs::write(&out, prettyplease::unparse(&file))
        .unwrap_or_else(|err| panic!("write {}: {err}", out.display()));
}
