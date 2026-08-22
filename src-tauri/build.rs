fn main() {
    println!("cargo:rerun-if-changed=../web/phone/src");
    tauri_build::build();
}
