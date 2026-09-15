//! Windows binary'sine ikon + sürüm bilgisi gömer.
//!
//! Yalnızca Windows'ta iş yapar; başka platformda derleme betiği boştur.
//! Gömme **başarısız olursa derleme kırılmaz** — uyarı basıp devam eder:
//! `cargo install tfs-ssh` yapan birinde Windows SDK'nın `rc.exe`si yoksa,
//! süsleme yüzünden kurulumun tamamen düşmesi kabul edilemez.

fn main() {
    #[cfg(windows)]
    windows_kaynaklari();
}

#[cfg(windows)]
fn windows_kaynaklari() {
    // Host Windows ama hedef değilse (çapraz derleme) kaynak gömülmez.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    println!("cargo:rerun-if-changed=assets/tfs.ico");

    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/tfs.ico");
    // Dosya özelliklerinde görünen alanlar. rc.exe kodlamasıyla uğraşmamak
    // için bilinçli olarak ASCII.
    res.set("ProductName", "tfs");
    res.set("FileDescription", "tfs - SSH terminal + SFTP file transfer");
    res.set("LegalCopyright", "MIT OR Apache-2.0");

    if let Err(e) = res.compile() {
        println!("cargo:warning=ikon gomulemedi ({e}); binary ikonsuz derlenecek");
    }
}
