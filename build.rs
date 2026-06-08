fn main() {
    println!("cargo:rerun-if-changed=remouseable-icon.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("remouseable-icon.ico");
        resource
            .compile()
            .expect("failed to compile Windows application resources");
    }
}
