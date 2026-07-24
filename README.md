# tfs (terminal-file-send) — SSH dosya tarayıcısı + modern SSH terminali

ratatui + russh + russh-sftp ile:
- **F2** — iki panelli (YEREL ↔ UZAK), fareyle sürükle-bırak SFTP dosya transferi,
- **F1** — PuTTY / Windows Terminal benzeri, tam ekran interaktif SSH terminali,

hepsi tek SSH bağlantısı üzerinden (yeniden bağlanma yok).

## Yapılandırma

Sunucular `config.json` dosyasından okunur — birden fazla sunucu, parolalarıyla:

```json
{
  "servers": [
    { "name": "Prod", "host": "1.2.3.4", "port": 22, "user": "okan", "password": "***" },
    { "name": "Test", "host": "test.local", "user": "root", "password": "***" }
  ]
}
```

- `port` opsiyonel (varsayılan 22).
- Şablon için `config.example.json` → `config.json` olarak kopyalayıp düzenleyin.
- **Güvenlik**: parolalar düz metin tutulur; `config.json`'ı repoya koymayın
  (`.gitignore`'a ekli). Anahtar (publickey) auth sonraki adımlarda.

## Çalıştırma

```powershell
cargo run                    # ./config.json okur
cargo run -- sunucular.json  # farklı bir config yolu
```

Açılışta sunucu listesi gelir → **fareyle tıkla = bağlan** (ya da ↑/↓ + Enter).

## İki mod: F1 Terminal · F2 Dosya

Bağlandıktan sonra aynı SSH bağlantısı üzerinde iki mod arasında geçiş yapılır
(yeniden bağlanma yok — tek bağlantı, iki kanal):

- **F1 → SSH Terminali**: PuTTY / Windows Terminal benzeri, tam ekran interaktif
  kabuk. ANSI/VT100 renkleri, imleç, `top`/`htop`/`vim` gibi tam ekran programlar
  çalışır. İlk F1'de kabuk açılır ve uygulama boyunca canlı kalır.
- **F2 → Dosya transferi**: iki panelli, fareyle sürükle-bırak SFTP tarayıcısı.

`F1`/`F2` **uygulama-global** kısayoldur; terminal modundayken kabuğa
iletilmezler (diğer tüm tuşlar kabuğa gider). Terminalden çıkmak için kabukta
`exit` yaz (dosya moduna döner) ya da `F2` → `q`.

**Yapıştırma**: Terminal modunda panodan yapıştırma desteklenir (terminalinizin
yapıştırma kısayolu — genelde `Ctrl+Shift+V`, `Shift+Insert` ya da sağ tık —
bracketed paste ile kabuğa iletilir).

**Terminal sorgu yanıtları**: Emülatör, kabuğun/promptun gönderdiği imleç-konumu
(CPR, `ESC[6n`) ve cihaz-kimliği (DA) sorgularına yanıt verir; aksi halde bazı
promptlar ~1 sn bekleyip ekranı sıfırlıyordu.

> Teşhis: `TFS_LOG=yol.txt` ortam değişkeniyle çalıştırırsanız sunucudan gelen ham
> baytlar escape'lenmiş olarak o dosyaya yazılır (terminal sorunlarını incelemek için).

## Kullanım

- **Sürükle-bırak**: Bir dosyayı bir panelden diğerine fareyle sürükleyip bırak
  → yükleme/indirme başlar. (YEREL→UZAK = upload, UZAK→YEREL = download.)
- **Tek tık**: dosya seçer; klasöre tıklamak içine girer.
- **Tekerlek**: seçimi kaydırır.
- **Klavye**: `Tab` panel değiştir, `Enter` gir, `Backspace` üst dizin, `↑/↓` gezin, `q` çıkış.
- **F1**: SSH terminaline geç · **F2**: dosya moduna dön.

## Sunucu seçme ekranı

- Açılışta `config.json`'daki sunucular listelenir.
- **Tek tık** ilgili sunucuya bağlanır; `↑/↓` + `Enter` de çalışır; `q` çıkar.

## Bilinen sınırlar (skeleton — sonraki adımlar)

- Sadece **dosya** transferi; klasör (recursive) desteği yok.
- Transferler artık ayrı bir tokio task'inde, parça parça (64 KiB) yapılır ve
  `mpsc` ile ortada bir **progress bar** gösterilir — UI bloklanmaz. Transfer
  sırasında `q`/`Esc` ile iptal edilebilir.
- Sunucu anahtarı doğrulanmıyor (`check_server_key` daima `true`).
  Prod'da `known_hosts` kontrolü ekle.
- Sadece parola kimlik doğrulaması; anahtar (publickey) auth eklenebilir.
- OS dosya yöneticisi ↔ terminal DnD **mümkün değil** (terminal sınırı).
- SSH terminali (F1): fare uzak programlara iletilmez; pano yapıştırma ve fareyle
  metin seçme henüz yok (sonraki adım). Kaydırma tamponu 1000 satır.

## Kripto backend notu

`russh` varsayılanı `aws-lc-rs` Windows'ta NASM ister; bu yüzden `Cargo.toml`'da
`ring` backend'i seçili (NASM gerektirmez).
