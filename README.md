# tfs (terminal-file-send) — SSH dosya tarayıcısı + modern SSH terminali

ratatui + russh + russh-sftp ile:
- **F2** — iki panelli (YEREL ↔ UZAK) SFTP dosya transferi: `t` ile ya da
  fareyle sürükle-bırak,
- **F1** — PuTTY / Windows Terminal benzeri, tam ekran interaktif SSH terminali,
- **F4** — seçili dosyayı [`fresh`](https://github.com/sinelaw/fresh) editöründe aç
  (uzak dosya: indir → düzenle → otomatik geri yükle),

hepsi tek SSH bağlantısı üzerinden (yeniden bağlanma yok).

## Kurulum

### Cargo ile (Rust kuruluysa)

```sh
cargo install tfs-ssh
```

> crates.io'da `tfs` adı başkasına ait olduğu için paket adı **`tfs-ssh`**;
> kurulan komut yine **`tfs`**'tir.

### Hazır sürümler (derlemeye gerek yok)

[**Releases**](https://github.com/okanaytimur/tfs/releases) sayfasından hazır
binary indirebilirsiniz:

| Dosya | Platform | Not |
|-------|----------|-----|
| `tfs-vX-linux-x86_64` | Linux x86_64 | glibc 2.34+ · indirdikten sonra `chmod +x` |
| `tfs-vX-windows-x86_64.exe` | Windows 10+ 64-bit | Önerilen |
| `tfs-vX-windows-i686.exe` | Windows 10+ 32-bit | Eski/32-bit Windows |

İndirdikten sonra yanına bir `config.json` koyup çalıştırın (bkz. Yapılandırma).

**Platform desteği**: Linux (glibc 2.34+ — Ubuntu 22.04+, Debian 12+, RHEL 9+)
ve Windows 10 ve üzeri. Windows 7/8 desteklenmez — Rust 1.78'den beri standart
Windows hedefi Win7'yi bırakmıştır (binary'ler Win10+ ister). Win7 ancak Tier-3
`*-win7-windows-msvc` hedefi + nightly + `-Z build-std` ile derlenebilir
(deneysel, kripto/async yığınımızla test edilmedi).

Linux'ta pano (kopyala/yapıştır) X11 ve Wayland'da çalışır, ek sistem
kütüphanesi gerekmez; binary yalnızca `libc`/`libm`/`libgcc_s`'e bağlıdır.
Kaynaktan derlemek için bir C linker gerekir (`dnf install gcc` /
`apt install build-essential`).

## Yapılandırma

**İlk çalıştırmada bir şey hazırlamanıza gerek yok**: `config.json` yoksa `tfs`
hata vermez, örnek bir tane oluşturur ve dosyanın tam yolunu gösterip çıkar.
Dosyayı açıp kendi sunucularınızı yazın, `tfs`'i tekrar çalıştırın.

```
$ tfs

tfs — ilk çalıştırma

Yapılandırma dosyası bulunamadı, sizin için örnek bir tane oluşturuldu:

    D:\isler\config.json

Dosyayı açıp kendi sunucularınızı yazın (name / host / port / user /
password), sonra tfs'i yeniden çalıştırın:

    notepad "D:\isler\config.json"
```

Dosya bulunduğunuz dizine oluşturulur; başka bir yol vermek için argüman
kullanın (`tfs sunucular.json` — gerekiyorsa alt dizinler de açılır).

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
- Şablonu elle de kopyalayabilirsiniz: `config.example.json` → `config.json`.
  (Dosya hiç düzenlenmemişse — yani birebir şablonsa — `tfs` bağlanmayı denemez,
  yine dosyayı düzenlemeniz gerektiğini söyler.)
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

### Terminal modunda kopyala / yapıştır / kaydır

| İşlem | Nasıl |
|-------|-------|
| **Kopyala** | Fareyle metnin üzerinden sürükle; tuşu bırakınca seçim panoya kopyalanır (üst çubukta `✓ N karakter kopyalandı`). |
| **Yapıştır** | **Sağ tık**, **Ctrl+V**, **Shift+Insert**, **Ctrl+Shift+V** ya da terminalinizin kendi Yapıştır menüsü — hepsi metni **tek seferde** gönderir. |
| **Geçmişe kaydır** | **Fare tekerleği** ya da **Shift+PgUp / Shift+PgDn**. Tampon 1000 satır; bir tuşa basınca canlı ekrana döner. |

Kaydırma sırasında üst çubukta kaç satır geride olduğunuz gösterilir ve imleç
gizlenir. Yapıştırılan metnin satır sonları `\r`'a çevrilir; uzak taraf bracketed
paste modundaysa (`vim`, `bash`) metin `ESC[200~`/`ESC[201~` ile sarılır — böylece
çok satırlı yapıştırma yanlışlıkla komut olarak çalışmaz ve `vim`'de otomatik
girinti metni bozmaz.

<details><summary>Windows'ta yapıştırma neden özel ele alınıyor?</summary>

crossterm'in `Event::Paste` olayı **Windows'ta hiç üretilmez** — eski konsol
API'sinde bracketed paste yok. Windows Terminal / conhost, `Ctrl+V` gibi
kısayolları kendisi yakalayıp panodaki metni uygulamaya **tek tek tuş olayı**
olarak enjekte eder. Bu yüzden yapıştırma harf harf gidiyordu: her karakter ayrı
bir SSH paketi (yavaş) ve `vim` bunu yazım sanıp otomatik girinti uyguluyordu.

Çözüm: bir tuş **hemen gönderilir** (yazmaya gecikme eklenmez), ardından
"arkasından devamı geliyor mu?" diye kısa bir süre (25 ms) dinlenir. Geliyorsa
bu bir yapıştırmadır; kalan karakterler toplanıp tek parça gönderilir. İnsan
yazımında tuşlar arası boşluk ≥60 ms, tuş tekrarında bile ≥32 ms olduğundan
sıradan yazma yığına dönüşmez. Ayrıca yalnızca **çok satırlı** yığın bracketed
paste ile sarılır — tek satırlıkta sarmanın bir faydası yok ve `vim` normal
modunda tuş tekrarı yanlışlıkla yığın sanılırsa zarar verebilirdi.

Sağ tık bu yolu hiç kullanmaz: panoyu doğrudan okur.

> Teşhis: `TFS_KEYLOG=tuslar.txt` ile çalıştırırsanız her girdi olayı, bir
> öncekinden kaç mikrosaniye sonra geldiğiyle birlikte kaydedilir — yapıştırmanın
> nasıl teslim edildiğini ölçmek için.

</details>

**Terminal sorgu yanıtları**: Emülatör, kabuğun/promptun gönderdiği imleç-konumu
(CPR, `ESC[6n`) ve cihaz-kimliği (DA) sorgularına yanıt verir; aksi halde bazı
promptlar ~1 sn bekleyip ekranı sıfırlıyordu.

> Teşhis: `TFS_LOG=yol.txt` ortam değişkeniyle çalıştırırsanız sunucudan gelen ham
> baytlar escape'lenmiş olarak o dosyaya yazılır (terminal sorunlarını incelemek için).

## F4 — dosyayı `fresh` ile düzenle

Dosya modunda (F2) bir dosya seçip **F4** (ya da **`e`**) tuşuna basınca dosya
[`fresh`](https://github.com/sinelaw/fresh) editöründe açılır — VS Code / Sublime
alışkanlıklarını terminale getiren, çoklu imleç ve LSP destekli bir editör.

| Panel | Ne olur |
|-------|---------|
| **YEREL** | Dosya olduğu yerde açılır. |
| **UZAK** | Dosya geçici bir dizine indirilir, editörde açılır; editör kapanınca **içeriği değiştiyse** SFTP ile geri yüklenir. Değişmediyse hiçbir şey yüklenmez. |

Uzak sunucuda `fresh` kurulu olmasına **gerek yoktur** — düzenleme her zaman
yerelde yapılır. Değişiklik tespiti içerik özetiyle (FNV-1a) yapılır, dosya
zaman damgasıyla değil: editör dosyayı açıp kaydetmeden kapansa da, aynı içeriği
tekrar yazsa da gereksiz yükleme olmaz. Uzak dosya sınırı **64 MiB**'tır (bu
sınır transferi içindir; `fresh` çok daha büyük dosyaları açabilir).

Editör açılırken tfs'in kendi arayüzü askıya alınır (ham mod kapatılır,
alternatif ekrandan çıkılır ve klavye olay akışı bırakılır — aksi halde tuşlar
editöre değil tfs'e giderdi); editör kapanınca arayüz geri gelir.

### `fresh` kurulu değilse

tfs onu sizin için kurmayı önerir:

```
┌ Editör kurulumu ─────────────────────────────────┐
│ fresh editörü bulunamadı.                        │
│                                                  │
│ Şimdi kurulsun mu? Çalıştırılacak:               │
│   $ cargo binstall --no-confirm fresh-editor     │
│                                                  │
│  E / Enter  kur     H / Esc  vazgeç              │
└──────────────────────────────────────────────────┘
```

`cargo binstall` hazır derlenmiş binary'yi indirir (saniyeler). Sırayla denenir:

1. `cargo binstall --no-confirm fresh-editor` — `cargo-binstall` kuruluysa.
2. Değilse önce `cargo install cargo-binstall`, sonra (1).
3. O da olmazsa son çare `cargo install --locked fresh-editor` (kaynaktan
   derler, uzun sürer).

Kurulum çıktısı doğrudan terminalde akar. Kurulu bir `fresh`i tfs, `PATH`'e ek
olarak `~/.cargo/bin` ve `~/.local/bin` altında da arar — böylece kurulumdan
hemen sonra, kabuk yeniden başlatılmadan bulunur.

Editörü elle kurmak isterseniz:

```sh
cargo binstall fresh-editor      # hazır binary
cargo install --locked fresh-editor   # kaynaktan
```

## Kullanım

- **`t`** (ya da **F5**): Odaklı paneldeki seçili dosyayı **karşı panele** aktarır
  — karşı panelin o anki dizinine, aynı adla. (YEREL odaklıysa upload, UZAK
  odaklıysa download.) Fare kullanmadan transfer.
- **Sürükle-bırak**: Bir dosyayı bir panelden diğerine fareyle sürükleyip bırak
  → yükleme/indirme başlar. (YEREL→UZAK = upload, UZAK→YEREL = download.)
- **Tek tık**: dosya seçer; klasöre tıklamak içine girer.
- **Tekerlek**: seçimi kaydırır.
- **Klavye**: `Tab` panel değiştir, `Enter` gir, `Backspace` üst dizin, `↑/↓` gezin,
  `t` transfer, `e` düzenle, `q` çıkış.
- **F1**: SSH terminaline geç · **F2**: dosya moduna dön. Üst çubuktaki
  `F1 Terminal` / `F2 Dosya` sekmelerine **fareyle de tıklanabilir**.
- **F4** (ya da **`e`**): seçili dosyayı `fresh` editöründe aç (bkz. yukarıdaki bölüm).

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
- SSH terminali (F1): fare uzak programlara **iletilmez** (SGR mouse forwarding
  yok) — `htop`/`vim` içinde fare çalışmaz, fare seçme/kopyalama içindir.
  Seçim satır bazlıdır (blok/dikdörtgen seçim yok).

## Kripto backend notu

`russh` varsayılanı `aws-lc-rs` Windows'ta NASM ister; bu yüzden `Cargo.toml`'da
`ring` backend'i seçili (NASM gerektirmez).
