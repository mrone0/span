fn main() {
    println!("cargo:rerun-if-changed=windows/span.rc");
    println!("cargo:rerun-if-changed=windows/Span.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("windows/span.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("could not embed the Windows application icon");
    }
}
