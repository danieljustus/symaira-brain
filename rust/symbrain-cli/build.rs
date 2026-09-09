fn main() {
    println!("cargo:rerun-if-env-changed=SYMBRAIN_VERSION");
    let version = std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map_or_else(|| "rustc".to_owned(), |version| version.trim().to_owned());
    println!("cargo:rustc-env=RUSTC_VERSION={version}");
}
