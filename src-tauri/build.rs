fn main() {
    println!("cargo:rerun-if-env-changed=TFROBOT_OFFICIAL_RELEASE");
    if std::env::var("TFROBOT_OFFICIAL_RELEASE").as_deref() == Ok("1") && tauri_build::is_dev() {
        panic!("Official release builds must enable Tauri custom-protocol (production-only Manager policy)");
    }
    tauri_build::build()
}
