//! Only the few messages the module says, from every catalog in po/.

use std::{env, fs, path::Path};

include!("../po/catalog.rs");

/// What say() can show. A test in src/lib.rs checks these are the ones.
const MESSAGES: &[&str] = &[
    "Look at the camera to unlock.",
    "Blink, or turn your head a little.",
    "Look at the screen.",
    "Move closer to the camera.",
    "It is too dark to see your face.",
    "Face unlock is paused after too many tries. Use your password.",
    "Face not recognised.",
];

fn main() {
    println!("cargo:rerun-if-changed=../po");
    println!("cargo:rerun-if-env-changed=FU_SOCKET");
    let mut code = String::from("#[allow(dead_code)]\npub(crate) const MESSAGES: &[&str] = &[\n");
    for m in MESSAGES {
        code.push_str(&format!("    {m:?},\n"));
    }
    code.push_str("];\npub(crate) static CATALOGS: &[(&str, &[(&str, &str)])] = &[\n");
    let mut files: Vec<_> = fs::read_dir("../po").map(|d| d.filter_map(Result::ok).map(|e| e.path()).collect()).unwrap_or_default();
    files.sort();
    for path in files {
        if path.extension().and_then(|e| e.to_str()) != Some("po") {
            continue;
        }
        let lang = path.file_stem().unwrap().to_string_lossy().to_string();
        let entries: Vec<(String, String)> = parse(&fs::read_to_string(&path).unwrap()).into_iter().filter(|(id, _)| MESSAGES.contains(&id.as_str())).collect();
        code.push_str(&format!("    ({lang:?}, &[\n"));
        for (id, text) in entries {
            code.push_str(&format!("        ({id:?}, {text:?}),\n"));
        }
        code.push_str("    ]),\n");
    }
    code.push_str("];\n");
    fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join("messages.rs"), code).unwrap();
}
