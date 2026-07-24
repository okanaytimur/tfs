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
#[derive(Clone, Copy, Debug)]
pub struct TransferProgress {
    /// Aktarılan bayt.
    pub done: u64,
    /// Toplam bayt (bilinmiyorsa 0).
    pub total: u64,
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

    /// Uzaktan yerele indir. Parça parça okur ve her chunk sonrası `tx` ile
    /// ilerleme bildirir; böylece UI bloklanmaz.
    pub async fn download(
        &self,
        remote_path: &str,
        local_path: &std::path::Path,
        tx: &mpsc::UnboundedSender<TransferProgress>,
    ) -> Result<()> {
        let total = self
            .sftp
            .metadata(remote_path)
            .await
            .ok()
            .and_then(|m| m.size)
            .unwrap_or(0);
        let mut remote = self.sftp.open(remote_path).await?;
        let mut local = tokio::fs::File::create(local_path).await?;

        let mut buf = vec![0u8; CHUNK];
        let mut done: u64 = 0;
        let _ = tx.send(TransferProgress { done, total });
        loop {
            let n = remote.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            local.write_all(&buf[..n]).await?;
            done += n as u64;
            let _ = tx.send(TransferProgress { done, total });
        }
        local.flush().await?;
        Ok(())
    }

    /// Yerelden uzağa yükle. Parça parça yazar ve her chunk sonrası `tx` ile
    /// ilerleme bildirir; böylece UI bloklanmaz.
    pub async fn upload(
        &self,
        local_path: &std::path::Path,
        remote_path: &str,
        tx: &mpsc::UnboundedSender<TransferProgress>,
    ) -> Result<()> {
        let total = tokio::fs::metadata(local_path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        let mut local = tokio::fs::File::open(local_path).await?;
        let mut remote = self.sftp.create(remote_path).await?;

        let mut buf = vec![0u8; CHUNK];
        let mut done: u64 = 0;
        let _ = tx.send(TransferProgress { done, total });
        loop {
            let n = local.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            remote.write_all(&buf[..n]).await?;
            done += n as u64;
            let _ = tx.send(TransferProgress { done, total });
        }
        remote.flush().await?;
        remote.shutdown().await?;
        Ok(())
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
