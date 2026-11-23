//! Compiles the gettext catalogs in po/ into the binary, so translations need
//! no files at run time and no libintl.

use std::{env, fs, path::Path};

include!("po/catalog.rs");

fn main() {
    // Where the Makefile installs things, read by src/paths.rs.
    for var in ["FU_LIBEXECDIR", "FU_MODELDIR", "FU_PAMDIR", "FU_BINDIR"] {
        println!("cargo:rerun-if-env-changed={var}");
    }
    println!("cargo:rerun-if-changed=po");
    let mut code = String::new();
    let mut langs = Vec::new();
    let mut files: Vec<_> = fs::read_dir("po")
        .map(|d| d.filter_map(Result::ok).map(|e| e.path()).collect())
        .unwrap_or_default();
    files.sort();
    for path in files {
        if path.extension().and_then(|e| e.to_str()) != Some("po") {
            continue;
        }
        println!("cargo:rerun-if-changed={}", path.display());
        let lang = path.file_stem().unwrap().to_string_lossy().to_string();
        let mut entries = parse(&fs::read_to_string(&path).unwrap());
        entries.sort();
        entries.dedup_by(|a, b| a.0 == b.0);
        let name = format!("CATALOG_{}", lang.to_uppercase().replace(['-', '@'], "_"));
        code.push_str(&format!("static {name}: &[(&str, &str)] = &[\n"));
        for (id, text) in &entries {
            code.push_str(&format!("    ({id:?}, {text:?}),\n"));
        }
        code.push_str("];\n");
        langs.push((lang, name));
    }
    code.push_str("pub(crate) static CATALOGS: &[(&str, &[(&str, &str)])] = &[\n");
    for (lang, name) in &langs {
        code.push_str(&format!("    ({lang:?}, {name}),\n"));
    }
    code.push_str("];\n");
    let out = Path::new(&env::var("OUT_DIR").unwrap()).join("catalogs.rs");
    fs::write(out, code).unwrap();
}
