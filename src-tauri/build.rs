use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../web");
    let dist_index = Path::new("../dist/index.html");
    let dist_js = Path::new("../dist/app.js");
    let dist_css = Path::new("../dist/index.css");

    if !dist_index.exists() || !dist_js.exists() || !dist_css.exists() {
        let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
        let status = Command::new(npm)
            .args(["run", "build"])
            .current_dir("..")
            .status()
            .expect("failed to run `npm run build` to prepare ../dist for Tauri");
        assert!(status.success(), "`npm run build` failed");
    }

    tauri_build::build()
}
