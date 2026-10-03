// Portable version probe fixture; compiled independently of Cargo coverage.
fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args != ["version", "--json"] {
        std::fs::write(std::env::var_os("SETUP_FALLBACK_RECEIPT").expect("unexpected fallback"), args.join("\n")).unwrap();
        std::process::exit(42);
    }
    println!("{{\"version\":\"0.0.0\"}}");
}
