fn main() {
    let phone = "web/phone";
    let status = std::process::Command::new("npm")
        .args(["run", "build"])
        .current_dir(phone)
        .status()
        .expect("failed to run `npm run build` in web/phone");
    assert!(status.success(), "frontend build failed");
    println!("cargo:rerun-if-changed=web/phone/src");
    tauri_build::build();
}
