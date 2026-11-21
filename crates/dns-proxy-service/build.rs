fn main() {
    let ico = std::path::Path::new("../../assets/dns-proxy.ico");
    if ico.exists() {
        embed_resource::compile("dns-proxy-service.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("failed to embed Windows icon resource");
        println!("cargo:rerun-if-changed=dns-proxy-service.rc");
        println!("cargo:rerun-if-changed=../../assets/dns-proxy.ico");
    }
}
