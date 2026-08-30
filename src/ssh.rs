//! russh tabanlı SSH bağlantısı + SFTP alt sistemi ve transfer yardımcıları.

use std::sync::Arc;

use anyhow::{bail, Context, Result};
use russh::client::{self, Handle};
use russh::keys::ssh_key;
use russh::Channel;
use russh_sftp::client::SftpSession;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

/// Transfer sırasında ana döngüye gönderilen ilerleme bilgisi.
#[derive(Clone, Debug, Default)]
pub struct TransferProgress {
    /// Aktarılan bayt (ağaç transferinde tüm dosyaların toplamı).
    pub done: u64,
    /// Toplam bayt (bilinmiyorsa 0).
    pub total: u64,
    /// Şu an ne yapılıyor: taranıyor ya da aktarılan dosyanın köke göre yolu.
    pub label: Option<String>,
    /// (tamamlanan, toplam) dosya sayısı — ağaç taraması bittikten sonra dolar.
    pub files: Option<(u32, u32)>,
}

/// Bir klasör (ya da tek dosya) transferinin sonucu.
#[derive(Clone, Debug, Default)]
pub struct TreeOutcome {
    pub files: u32,
    pub dirs: u32,
    /// Atlanan sembolik bağ sayısı (bkz. `walk_local`).
    pub symlinks: u32,
    /// Listelenemeyen dizinler (izin yok vb.) — atlandı.
    pub unreadable: u32,
    /// Aktarılamayan dosya sayısı.
    pub errors: u32,
    /// İlk hatanın metni (durum çubuğunda gösterilir).
    pub first_error: Option<String>,
}

impl TreeOutcome {
    /// Durum çubuğu için insan-okur özet: "12 dosya · 3 klasör · 1 hata".
    pub fn summary(&self) -> String {
        let mut parts = vec![format!("{} dosya", self.files)];
        if self.dirs > 0 {
            parts.push(format!("{} klasör", self.dirs));
        }
        if self.symlinks > 0 {
            parts.push(format!("{} sembolik bağ atlandı", self.symlinks));
        }
        if self.unreadable > 0 {
            parts.push(format!("{} dizin okunamadı", self.unreadable));
        }
        if self.errors > 0 {
            parts.push(format!("{} hata", self.errors));
        }
        parts.join(" · ")
    }
}

/// Aktarılacak ağaçtaki tek kalem.
#[derive(Clone, Debug)]
struct TreeItem {
    /// Köke göre yol (`/` ile ayrılır). Kökün kendisi için boş.
    rel: String,
    kind: Kind,
    /// Dosyaysa boyutu.
    size: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Dir,
    File,
    /// Sembolik bağlar kopyalanmaz: dizin bağları tarama döngüsüne yol açar ve
    /// bağı hedefiyle değiştirmek sessiz bir sürpriz olurdu. Sayılıp bildirilir.
    Symlink,
}

/// Tarama sonucu: ne aktarılacak, ne kadar tutuyor.
#[derive(Debug, Default)]
struct Tree {
    items: Vec<TreeItem>,
    total_bytes: u64,
    files: u32,
    dirs: u32,
    symlinks: u32,
    unreadable: u32,
}

impl Tree {
    fn push(&mut self, rel: String, kind: Kind, size: u64) {
        match kind {
            Kind::Dir => self.dirs += 1,
            Kind::File => {
                self.files += 1;
                self.total_bytes += size;
            }
            Kind::Symlink => self.symlinks += 1,
        }
        self.items.push(TreeItem { rel, kind, size });
    }
}

/// İlerlemeyi biriktirip UI'a bildiren yardımcı. Çubuğun **monoton** kalması
/// için dosya bitiminde sayaç, gerçekte kaç bayt okunduğuna değil dosyanın
/// beklenen boyutuna sabitlenir (hata hâlinde de ilerleme geri gitmez).
struct Reporter<'a> {
    tx: &'a mpsc::UnboundedSender<TransferProgress>,
    done: u64,
    total: u64,
    files_done: u32,
    files_total: u32,
    label: Option<String>,
    /// İçinde bulunduğumuz dosya başlarkenki `done` değeri.
    file_base: u64,
}

impl<'a> Reporter<'a> {
    fn new(tx: &'a mpsc::UnboundedSender<TransferProgress>, tree: &Tree) -> Self {
        let me = Self {
            tx,
            done: 0,
            total: tree.total_bytes,
            files_done: 0,
            files_total: tree.files,
            label: None,
            file_base: 0,
        };
        me.send();
        me
    }

    fn send(&self) {
        let _ = self.tx.send(TransferProgress {
            done: self.done,
            total: self.total,
            label: self.label.clone(),
            files: Some((self.files_done, self.files_total)),
        });
    }

    fn start_file(&mut self, rel: &str) {
        self.file_base = self.done;
        self.label = Some(rel.to_string());
        self.send();
    }

    fn bump(&mut self, n: u64) {
        self.done += n;
        self.send();
    }

    fn finish_file(&mut self, size: u64) {
        self.done = self.file_base + size;
        self.files_done += 1;
        self.send();
    }
}

/// Chunk boyutu (64 KiB) — her chunk sonrası ilerleme bildirilir.
const CHUNK: usize = 64 * 1024;

/// Sunucu anahtarını sorgusuz kabul eden basit handler (skeleton için).
/// Prod'da known_hosts doğrulaması eklenmeli.
struct ClientHandler;

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(&mut self, _key: &ssh_key::PublicKey) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// Uzak dosya sistemiyle konuşan oturum. `handle` bağlantıyı canlı tutar ve
/// aynı bağlantı üzerinde ikinci bir kanal (interaktif kabuk) açmak için kullanılır.
pub struct Ssh {
    handle: Handle<ClientHandler>,
    pub sftp: SftpSession,
}

/// Uzak dizindeki bir girdi.
#[derive(Clone, Debug)]
pub struct RemoteEntry {
    pub name: String,
    pub is_dir: bool,
}

impl Ssh {
    /// Parola ile bağlanır ve SFTP alt sistemini açar.
    pub async fn connect(host: &str, port: u16, user: &str, pass: &str) -> Result<Self> {
        let config = Arc::new(client::Config::default());
        let mut handle = client::connect(config, (host, port), ClientHandler)
            .await
            .with_context(|| format!("{host}:{port} bağlantısı kurulamadı"))?;

        let auth = handle
            .authenticate_password(user, pass)
            .await
            .context("kimlik doğrulama isteği başarısız")?;
        if !auth.success() {
            bail!("kimlik doğrulama reddedildi (kullanıcı/parola)");
        }

        let channel = handle.channel_open_session().await?;
        channel.request_subsystem(true, "sftp").await?;
        let sftp = SftpSession::new(channel.into_stream())
            .await
            .context("SFTP alt sistemi başlatılamadı")?;

        Ok(Self { handle, sftp })
    }

    /// Bağlanılan andaki geçerli uzak dizin (ör. kullanıcı home).
    pub async fn home(&self) -> Result<String> {
        Ok(self.sftp.canonicalize(".").await.unwrap_or_else(|_| "/".into()))
    }

    /// Aynı SSH bağlantısı üzerinde yeni bir kanal açıp PTY + interaktif kabuk
    /// ister. Dönen kanaldan `wait()` ile veri okunur, `data()` ile tuş yazılır.
    /// `cols`/`rows` başlangıç terminal boyutudur (sonradan `window_change` ile
    /// güncellenebilir).
    pub async fn open_shell(&self, cols: u16, rows: u16) -> Result<Channel<client::Msg>> {
        let channel = self
            .handle
            .channel_open_session()
            .await
            .context("kabuk kanalı açılamadı")?;
        channel
            .request_pty(true, "xterm-256color", cols as u32, rows as u32, 0, 0, &[])
            .await
            .context("PTY isteği başarısız")?;
        channel
            .request_shell(true)
            .await
            .context("kabuk (shell) isteği başarısız")?;
        Ok(channel)
    }

    /// Uzak dizini listeler. `..` girişini de en başa ekler.
    pub async fn list_dir(&self, path: &str) -> Result<Vec<RemoteEntry>> {
        let mut out = vec![RemoteEntry {
            name: "..".into(),
            is_dir: true,
        }];
        let rd = self
            .sftp
            .read_dir(path)
            .await
            .with_context(|| format!("uzak dizin okunamadı: {path}"))?;
        for entry in rd {
            out.push(RemoteEntry {
                name: entry.file_name(),
                is_dir: entry.file_type().is_dir(),
            });
        }
        out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
        Ok(out)
    }

    // --- Transfer (tek dosya ya da klasör ağacı) ---

    /// Uzaktan yerele indirir. Yol bir **klasörse ağacın tamamı** inilir.
    ///
    /// Önce ağaç taranır (böylece ilerleme çubuğu gerçek bir toplam gösterir),
    /// sonra sırayla dizinler açılıp dosyalar kopyalanır. Tek bir dosyanın
    /// hatası transferi durdurmaz; sayılır ve sonunda bildirilir.
    pub async fn download(
        &self,
        remote_root: &str,
        local_root: &std::path::Path,
        tx: &mpsc::UnboundedSender<TransferProgress>,
    ) -> Result<TreeOutcome> {
        let _ = tx.send(TransferProgress {
            label: Some("taranıyor…".into()),
            ..Default::default()
        });
        let tree = self.walk_remote(remote_root).await?;

        let mut out = TreeOutcome {
            symlinks: tree.symlinks,
            unreadable: tree.unreadable,
            ..Default::default()
        };
        let mut rep = Reporter::new(tx, &tree);

        for item in &tree.items {
            let src = remote_child(remote_root, &item.rel);
            let dst = local_child(local_root, &item.rel);
            match item.kind {
                Kind::Symlink => {}
                Kind::Dir => match tokio::fs::create_dir_all(&dst).await {
                    Ok(()) => out.dirs += 1,
                    Err(e) => out.note_error(&item.rel, &e.to_string()),
                },
                Kind::File => {
                    rep.start_file(&item.rel);
                    match self.copy_down(&src, &dst, &mut rep).await {
                        Ok(()) => out.files += 1,
                        Err(e) => out.note_error(&item.rel, &e.to_string()),
                    }
                    rep.finish_file(item.size);
                }
            }
        }
        Ok(out)
    }

    /// Yerelden uzağa yükler. Yol bir **klasörse ağacın tamamı** çıkılır.
    pub async fn upload(
        &self,
        local_root: &std::path::Path,
        remote_root: &str,
        tx: &mpsc::UnboundedSender<TransferProgress>,
    ) -> Result<TreeOutcome> {
        let _ = tx.send(TransferProgress {
            label: Some("taranıyor…".into()),
            ..Default::default()
        });
        let tree = walk_local(local_root)?;

        let mut out = TreeOutcome {
            symlinks: tree.symlinks,
            unreadable: tree.unreadable,
            ..Default::default()
        };
        let mut rep = Reporter::new(tx, &tree);

        for item in &tree.items {
            let src = local_child(local_root, &item.rel);
            let dst = remote_child(remote_root, &item.rel);
            match item.kind {
                Kind::Symlink => {}
                Kind::Dir => match self.ensure_remote_dir(&dst).await {
                    Ok(()) => out.dirs += 1,
                    Err(e) => out.note_error(&item.rel, &e.to_string()),
                },
                Kind::File => {
                    rep.start_file(&item.rel);
                    match self.copy_up(&src, &dst, &mut rep).await {
                        Ok(()) => out.files += 1,
                        Err(e) => out.note_error(&item.rel, &e.to_string()),
                    }
                    rep.finish_file(item.size);
                }
            }
        }
        Ok(out)
    }

    /// Tek dosya: uzaktan yerele, parça parça.
    async fn copy_down(
        &self,
        remote: &str,
        local: &std::path::Path,
        rep: &mut Reporter<'_>,
    ) -> Result<()> {
        let mut src = self.sftp.open(remote).await?;
        let mut dst = tokio::fs::File::create(local).await?;
        let mut buf = vec![0u8; CHUNK];
        loop {
            let n = src.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            dst.write_all(&buf[..n]).await?;
            rep.bump(n as u64);
        }
        dst.flush().await?;
        Ok(())
    }

    /// Tek dosya: yerelden uzağa, parça parça.
    async fn copy_up(
        &self,
        local: &std::path::Path,
        remote: &str,
        rep: &mut Reporter<'_>,
    ) -> Result<()> {
        let mut src = tokio::fs::File::open(local).await?;
        let mut dst = self.sftp.create(remote).await?;
        let mut buf = vec![0u8; CHUNK];
        loop {
            let n = src.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            dst.write_all(&buf[..n]).await?;
            rep.bump(n as u64);
        }
        dst.flush().await?;
        dst.shutdown().await?;
        Ok(())
    }

    /// Uzak dizini yoksa açar. Zaten varsa (ya da aramızda oluşturulduysa)
    /// başarı sayılır — `create_dir` var olan dizinde hata döner.
    async fn ensure_remote_dir(&self, path: &str) -> Result<()> {
        if self.sftp.try_exists(path).await.unwrap_or(false) {
            return Ok(());
        }
        match self.sftp.create_dir(path).await {
            Ok(()) => Ok(()),
            Err(e) => {
                if self.sftp.try_exists(path).await.unwrap_or(false) {
                    Ok(())
                } else {
                    Err(anyhow::Error::new(e))
                        .with_context(|| format!("uzak dizin açılamadı: {path}"))
                }
            }
        }
    }

    /// Uzak ağacı genişlik-öncelikli tarar. Sıralama önemli: bir dizin daima
    /// içindekilerden **önce** listeye girer, böylece hedefte dizinler
    /// içerikleri gelmeden açılmış olur.
    async fn walk_remote(&self, root: &str) -> Result<Tree> {
        let meta = self
            .sftp
            .symlink_metadata(root)
            .await
            .with_context(|| format!("uzak yol okunamadı: {root}"))?;

        let mut tree = Tree::default();
        let ft = meta.file_type();
        if ft.is_symlink() {
            tree.push(String::new(), Kind::Symlink, 0);
            return Ok(tree);
        }
        if !ft.is_dir() {
            tree.push(String::new(), Kind::File, meta.len());
            return Ok(tree);
        }

        tree.push(String::new(), Kind::Dir, 0);
        let mut queue = std::collections::VecDeque::from([String::new()]);
        while let Some(rel) = queue.pop_front() {
            let dir = remote_child(root, &rel);
            let entries = match self.sftp.read_dir(&dir).await {
                Ok(e) => e,
                // İzin yok gibi durumlarda tüm transferi düşürme; say ve devam et.
                Err(_) => {
                    tree.unreadable += 1;
                    continue;
                }
            };
            for entry in entries {
                let name = entry.file_name();
                if name == "." || name == ".." {
                    continue;
                }
                let child = join_rel(&rel, &name);
                let ft = entry.file_type();
                if ft.is_symlink() {
                    tree.push(child, Kind::Symlink, 0);
                } else if ft.is_dir() {
                    tree.push(child.clone(), Kind::Dir, 0);
                    queue.push_back(child);
                } else {
                    tree.push(child, Kind::File, entry.metadata().len());
                }
            }
        }
        Ok(tree)
    }
}

/// Yerel ağacı genişlik-öncelikli tarar (bkz. `Ssh::walk_remote`).
///
/// Sembolik bağlar **izlenmez**: dizin bağı taramayı sonsuz döngüye sokabilir
/// ve bağı hedefiyle değiştirmek sessiz bir sürpriz olurdu. Sayılır, bildirilir.
fn walk_local(root: &std::path::Path) -> Result<Tree> {
    let meta = std::fs::symlink_metadata(root)
        .with_context(|| format!("yerel yol okunamadı: {}", root.display()))?;

    let mut tree = Tree::default();
    let ft = meta.file_type();
    if ft.is_symlink() {
        tree.push(String::new(), Kind::Symlink, 0);
        return Ok(tree);
    }
    if !ft.is_dir() {
        tree.push(String::new(), Kind::File, meta.len());
        return Ok(tree);
    }

    tree.push(String::new(), Kind::Dir, 0);
    let mut queue = std::collections::VecDeque::from([String::new()]);
    while let Some(rel) = queue.pop_front() {
        let dir = local_child(root, &rel);
        let rd = match std::fs::read_dir(&dir) {
            Ok(r) => r,
            Err(_) => {
                tree.unreadable += 1;
                continue;
            }
        };
        for de in rd.flatten() {
            let name = de.file_name().to_string_lossy().to_string();
            let child = join_rel(&rel, &name);
            let ft = match de.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            if ft.is_symlink() {
                tree.push(child, Kind::Symlink, 0);
            } else if ft.is_dir() {
                tree.push(child.clone(), Kind::Dir, 0);
                queue.push_back(child);
            } else {
                let size = de.metadata().map(|m| m.len()).unwrap_or(0);
                tree.push(child, Kind::File, size);
            }
        }
    }
    Ok(tree)
}

impl TreeOutcome {
    fn note_error(&mut self, rel: &str, msg: &str) {
        self.errors += 1;
        if self.first_error.is_none() {
            let what = if rel.is_empty() { "kök" } else { rel };
            self.first_error = Some(format!("{what}: {msg}"));
        }
    }
}

/// Köke göre yolları birleştirir (`/` ayırıcı, boş kök kısayolu).
fn join_rel(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

/// Uzak kök + köke göre yol. `rel` boşsa kökün kendisi.
fn remote_child(root: &str, rel: &str) -> String {
    if rel.is_empty() {
        root.to_string()
    } else {
        remote_join(root, rel)
    }
}

/// Yerel kök + köke göre yol. `rel` boşsa kökün kendisi.
fn local_child(root: &std::path::Path, rel: &str) -> std::path::PathBuf {
    if rel.is_empty() {
        root.to_path_buf()
    } else {
        // `rel` daima `/` ile ayrılır; Windows'ta da `join` doğru çalışsın diye
        // parça parça ekleniyor.
        rel.split('/').fold(root.to_path_buf(), |acc, part| acc.join(part))
    }
}

/// Uzak yollar daima `/` ile birleştirilir.
pub fn remote_join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// `..` uygulanmış hâlde bir üst uzak dizin.
pub fn remote_parent(dir: &str) -> String {
    let trimmed = dir.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) | None => "/".into(),
        Some(idx) => trimmed[..idx].to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Testler için benzersiz, kendi kendini toplayan bir dizin.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            use std::sync::atomic::{AtomicU32, Ordering};
            static SEQ: AtomicU32 = AtomicU32::new(0);
            let p = std::env::temp_dir().join(format!(
                "tfs-ssh-test-{}-{tag}-{}",
                std::process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn write(p: &std::path::Path, bytes: &[u8]) {
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(p, bytes).unwrap();
    }

    #[test]
    fn tek_dosya_tek_kalem_olur() {
        let t = TempDir::new("tek");
        let f = t.path().join("a.txt");
        write(&f, b"12345");

        let tree = walk_local(&f).unwrap();
        assert_eq!(tree.items.len(), 1);
        assert_eq!(tree.items[0].rel, "");
        assert_eq!(tree.items[0].kind, Kind::File);
        assert_eq!(tree.total_bytes, 5);
        assert_eq!(tree.files, 1);
        assert_eq!(tree.dirs, 0);
    }

    #[test]
    fn agac_taranir_ve_toplam_boyut_dogru() {
        let t = TempDir::new("agac");
        write(&t.path().join("kok.txt"), b"aa");
        write(&t.path().join("alt/bir.txt"), b"bbb");
        write(&t.path().join("alt/derin/iki.txt"), b"cccc");

        let tree = walk_local(t.path()).unwrap();
        assert_eq!(tree.files, 3);
        // kök + alt + alt/derin
        assert_eq!(tree.dirs, 3);
        assert_eq!(tree.total_bytes, 2 + 3 + 4);
    }

    /// Hedefte dizinler içerikleri gelmeden açılmalı: bir dizin listede daima
    /// altındaki her kalemden **önce** gelmeli. Transferin doğruluğu buna bağlı.
    #[test]
    fn dizinler_iceriklerinden_once_gelir() {
        let t = TempDir::new("sira");
        write(&t.path().join("a/b/c/derin.txt"), b"x");
        write(&t.path().join("a/yuzey.txt"), b"y");

        let tree = walk_local(t.path()).unwrap();
        let mut gorulen_dizinler: Vec<&str> = Vec::new();
        for it in &tree.items {
            if let Some((parent, _)) = it.rel.rsplit_once('/') {
                assert!(
                    gorulen_dizinler.contains(&parent),
                    "{} , ebeveyni {} listeye girmeden geldi",
                    it.rel,
                    parent
                );
            }
            if it.kind == Kind::Dir {
                gorulen_dizinler.push(&it.rel);
            }
        }
    }

    /// Sembolik bağlar izlenmez (dizin bağı taramayı döngüye sokardı), sayılır.
    #[cfg(unix)]
    #[test]
    fn sembolik_baglar_izlenmez_sayilir() {
        let t = TempDir::new("baglar");
        write(&t.path().join("gercek/dosya.txt"), b"z");
        std::os::unix::fs::symlink(t.path().join("gercek"), t.path().join("dongu")).unwrap();

        let tree = walk_local(t.path()).unwrap();
        assert_eq!(tree.symlinks, 1);
        // Bağ izlenseydi dosya.txt iki kez sayılırdı.
        assert_eq!(tree.files, 1);
        assert!(tree.items.iter().any(|i| i.rel == "dongu" && i.kind == Kind::Symlink));
    }

    #[test]
    fn kok_yollari_bos_rel_ile_kokun_kendisi() {
        assert_eq!(remote_child("/srv/veri", ""), "/srv/veri");
        assert_eq!(remote_child("/srv/veri", "alt/a.txt"), "/srv/veri/alt/a.txt");
        assert_eq!(local_child(std::path::Path::new("/tmp/k"), ""), PathBuf::from("/tmp/k"));
        assert_eq!(
            local_child(std::path::Path::new("/tmp/k"), "alt/a.txt"),
            PathBuf::from("/tmp/k").join("alt").join("a.txt")
        );
    }

    #[test]
    fn join_rel_bos_ebeveyni_atlar() {
        assert_eq!(join_rel("", "a"), "a");
        assert_eq!(join_rel("a", "b"), "a/b");
    }

    #[test]
    fn ozet_yalnizca_dolu_alanlari_yazar() {
        let sade = TreeOutcome { files: 3, ..Default::default() };
        assert_eq!(sade.summary(), "3 dosya");

        let karisik = TreeOutcome {
            files: 3,
            dirs: 2,
            symlinks: 1,
            errors: 1,
            ..Default::default()
        };
        assert_eq!(
            karisik.summary(),
            "3 dosya · 2 klasör · 1 sembolik bağ atlandı · 1 hata"
        );
    }
}
