# PLAN — F1: SSH Terminali + F2: Dosya Transferi

> Bu dosya **oturumlar arası devam** içindir. Yeni bir oturum açtığında önce
> bunu oku; "Durum" bölümündeki kutucuklardan (`[ ]` / `[x]`) nerede kaldığımızı
> gör ve "Sıradaki adım"dan devam et.

Son güncelleme: 2026-07-31

> **DURUM: Aşama 0–4 + bug düzeltmeleri tamam, çalışır durumda (clippy temiz).**
> F1 SSH terminali + F2 dosya transferi entegre edildi.
>
> **2026-07-25 yapılanlar:**
> - ✅ BUG düzeltildi: tek-satır-komut sonrası ekran temizlenmesi → terminal
>   sorgu yanıtları (CPR `ESC[6n`, DSR `ESC[5n`, DA `ESC[c`) eklendi (`terminal.rs`
>   `scan_queries`). Kök neden: emülatör bu sorgulara yanıt vermeyince prompt
>   ~1 sn timeout bekleyip ekranı sıfırlıyordu.
> - ✅ Yapıştırma (bracketed paste): `EnableBracketedPaste` + `Event::Paste`.
> - ✅ Toplu besleme (döngü başında `try_recv` drain) — büyük çıktıda az redraw.
> - ✅ Teşhis logu: `TFS_LOG=yol` env → gelen ham baytlar escape'li loglanır.
> - ✅ GitHub'a atıldı (PUBLIC): https://github.com/okanaytimur/tfs
>   Paket `sftp_tui` → **tfs** olarak yeniden adlandırıldı (binary `tfs.exe`).
>   Güvenlik: `config.json` gitignore'lu, uzakta OLMADIĞI doğrulandı.
> - ✅ Fareyle sekme geçişi (üst çubuktaki F1/F2 tıklanabilir).
> - ✅ 32-bit (i686) + 64-bit (x86_64) Windows release derlendi → GitHub Releases.
> - ⛔ Windows 7/8: bilinçli olarak desteklenmiyor (Rust 1.78+ bıraktı; bkz. "## 10.2").
> - ✅ crates.io'da YAYINDA: `cargo install tfs-ssh` → `tfs` komutu (2026-07-27, doğrulandı).
>
> **2026-07-31 yapılanlar (Aşama 5 TAMAMLANDI, commit'lenmedi):**
> - ✅ **Fareyle metin seçme + kopyalama**: sürükle → REVERSED vurgu → bırakınca
>   `arboard` ile panoya. `terminal.rs`: `Selection`, `sel_start/sel_update/
>   sel_finish_copy`, `selection_span`, `highlight_selection`.
> - ✅ **Panodan yapıştırma**: sağ tık · `Shift+Insert` · `Ctrl+Shift+V`
>   (`paste_from_clipboard`). `Ctrl+V` bilinçli olarak kabuğa gidiyor.
> - ✅ **`send_paste`**: satır sonu `\r` normalizasyonu + uzak taraf bracketed
>   paste modundaysa `ESC[200~/201~` sarmalama. `Event::Paste` de bunu kullanıyor.
> - ✅ **Kaydırma tamponu artık erişilebilir**: fare tekerleği + `Shift+PgUp/PgDn`
>   (`scroll_up/scroll_down/scroll_reset`). 1000 satırlık `SCROLLBACK` şimdiye
>   kadar ölü sermayeydi. Kaydırınca imleç gizlenir, üst çubukta konum yazar,
>   tuşa basınca canlıya döner.
> - ✅ `cargo clippy --all-targets` temiz.
>
> **2026-08-01 (kullanıcı testi sonrası düzeltmeler):**
> - Kullanıcı doğruladı: **sağ tık yapıştırma ve seçimle kopyalama ÇALIŞIYOR.**
> - 🐞 **Ctrl+V / Ctrl+Shift+V / Shift+Ins harf harf yapıştırıyordu** → ÇÖZÜLDÜ.
>   Kök neden: crossterm'in `Event::Paste`'i **Windows'ta hiç üretilmez** (eski
>   konsol API'sinde bracketed paste yok; `EnableBracketedPaste::execute_winapi`
>   düpedüz `Unsupported` döner). Windows Terminal bu kısayolları kendisi yakalayıp
>   panoyu tuş tuş enjekte ediyor. Çözüm: `main.rs::drain_key_burst` — ilk
>   yazılabilir tuştan sonra akışta hazır bekleyen tuşlar toplanıp tek parça
>   `send_paste` ile gönderiliyor (bracketed paste sarmalı dahil).
>   Eşikler: `BURST_START_GAP` 2 ms, `BURST_GAP` 15 ms, `BURST_MAX_BYTES` 64 KiB.
>   İnsan yazımı (≥60 ms, tuş tekrarı ≥30 ms) asla yığın sayılmaz.
> - ✅ **`config.json` otomatik oluşuyor**: yoksa hata yerine şablon yazılıp tam
>   yol gösteriliyor ve nazikçe çıkılıyor (`Config::load_or_create` → `Loaded`
>   enum'u; şablon `include_str!("../config.example.json")` ile gömülü, bu yüzden
>   `cargo install` ile kurulanda da var). Dosya var ama birebir şablonsa da
>   bağlanmayı denemeyip kullanıcıyı dosyaya yönlendiriyor. Alt dizinli yol
>   verilirse dizin de açılıyor.
> - ✅ **İlk testler eklendi** (`main.rs` `mod tests`, 9 test): yığın toplama
>   mantığı her tuş vuruşunun yolunda olduğu için zamanlamaya duyarlı kısmı
>   `tokio::test(start_paused)` ile sanal zamanda doğrulanıyor.
>   `drain_key_burst` bunun için akış üzerinden generic yapıldı.
>   Yeni dev-dependency: `tokio` `test-util` (`full` bunu içermiyor).
> - Elle test edildi: config'in üç hâli (yok / dokunulmamış şablon / bozuk JSON)
>   + alt dizinli yol. **Yapıştırma yığını gerçek sunucuda test EDİLMEDİ.**
>
> **2026-08-01 (2. tur — ilk yığın denemesi ÇALIŞMADI, yeniden tasarlandı):**
> Kullanıcı test etti: sağ tık hâlâ iyi, ama Ctrl+V/Ctrl+Shift+V/Shift+Ins hâlâ
> harf harf. İlk tasarımın iki ayrı hatası vardı:
> - 🐞 **`now_or_never()` crossterm `EventStream` ile KULLANILMAZ.** `poll_next`
>   Pending dönerken `cx.waker()`'ı saklayıp bir thread'e veriyor ve
>   `stream_wake_task_executed` bayrağını set ediyor (stream.rs:113-131).
>   Noop waker verirsek bayrak takılı kalır, **gerçek waker bir daha
>   kaydedilemez** → ana döngü tuşlara sağır kalabilir (pratikte kabuk echo'su
>   uyandırdığı için maskeleniyordu). Artık her yoklama `timeout(...)` ile,
>   yani gerçek waker'la yapılıyor. Olay hazırsa zaten beklemeden dönüyor.
> - 🐞 **Yığın "başlatma" eşiği 2 ms fazla dardı.** Emülatör karakterleri tek
>   seferde tamponlamak yerine damla damla enjekte ediyorsa ikinci karakter
>   2 ms içinde gelmiyor ve yığın hiç başlamıyordu. Yeni tasarım eşiği tek ve
>   cömert: `BURST_GAP` = 25 ms (tuş tekrarının 32 ms'lik tabanının altında).
> - Yeni akış: tuş **hemen** gönderilir (yazmaya gecikme yok), sonra devamı
>   dinlenir → `collect_key_burst` → `TermSession::send_burst`.
> - `send_burst` yalnızca **çok satırlı** yığını bracketed paste ile sarar:
>   `vim` normal modunda bracketed paste "yapıştır" demek, dolayısıyla tuş
>   tekrarı yanlışlıkla yığın sanılsa bile `jjjj` metin olarak yapışmaz.
> - ✅ Teşhis eklendi: **`TFS_KEYLOG=yol`** → her girdi olayı, öncekinden kaç
>   mikrosaniye sonra geldiğiyle loglanır + her yığının kaç karakter topladığı.
>   **Hâlâ harf harf ise ilk iş bu logu almak** — gerçek enjeksiyon aralığını
>   gösterir, `BURST_GAP` ona göre ayarlanır.
> - 10 test (yeni: damla damla gelen yapıştırma da toparlanmalı).
> - ✅ **Kullanıcı doğruladı: yapıştırma artık tek seferde çalışıyor.**
>   Demek ki kök neden 2 ms'lik dar eşik + noop-waker'dı (ikisi de düzeltildi).
>
> **2026-08-01 (3. tur — v0.2.0 yayını):** Sürüm `0.2.0`'a çıkarıldı.
> `cargo package` doğrulandı: `config.example.json` pakete giriyor ve paketlenmiş
> crate tek başına derleniyor (`include_str!` bağımlılığı için kritik).
> Yayın komutları kullanıcıya verildi (kullanıcı kendisi çalıştırıyor).
> 📌 SONRAKİ: Linux sürümü (bkz "## 11").
>
> Kalan öneriler → "## 9.C". Derleme/dağıtım → "## 10". Devam için ilgili bölüme bak.

---

## 1. Hedef

Tek uygulama, iki mod. Bağlanılan sunucuyu **hem dosya hem de kabuk (shell)**
üzerinden yönetmek:

- **F2 → Dosya Transferi**: Mevcut iki panelli, fareyle sürükle-bırak SFTP
  tarayıcısı (halihazırda çalışıyor).
- **F1 → SSH Terminali**: PuTTY / Windows Terminal benzeri, modern, tam ekran
  interaktif kabuk. ANSI/VT100 renkleri, imleç, temizleme, `top`/`htop`/`vim`
  gibi tam ekran TUI programları çalışabilecek.

İki mod **aynı SSH bağlantısını paylaşır** (yeniden bağlanma yok). Mod
değiştirme anında olur; terminal oturumu arka planda canlı kalır.

---

## 2. Neden tek bağlantı, iki kanal (mimari karar)

SSH protokolü tek bağlantı üzerinde birden çok "channel" açmaya izin verir.
`russh` bunu destekler. Yani:

```
        ┌──────────────── SSH Handle (tek bağlantı) ────────────────┐
        │                                                           │
   channel 1: subsystem "sftp"  ──► SftpSession   (F2 dosya modu)   │
   channel 2: pty + shell       ──► interaktif kabuk (F1 terminal)  │
        └───────────────────────────────────────────────────────────┘
```

Avantaj: tek kimlik doğrulama, tek soket, iki modu ayrı ayrı yeniden
bağlanmadan kullanma. Kabuk kanalı bir kez açılıp uygulama boyunca canlı tutulur
(F1'e her geçişte yeni kabuk açmayız → oturum/çalışan program korunur).

---

## 3. Teknik parçalar

### 3.1 PTY + shell kanalı (russh)
- `handle.channel_open_session()` ile yeni kanal.
- `channel.request_pty(true, term, cols, rows, 0, 0, &modes)` — `term = "xterm-256color"`.
- `channel.request_shell(true)`.
- Okuma: `channel.wait().await` → `ChannelMsg::Data { data }` (stdout) ve
  `ChannelMsg::ExtendedData { data, ext }` (stderr) mesajlarını al.
- Yazma: kullanıcı tuşlarını `channel.data(&bytes[..]).await` ile gönder
  (ya da `channel.make_writer()` ile AsyncWrite).
- Yeniden boyutlandırma: terminal boyutu değişince
  `channel.window_change(cols, rows, 0, 0).await`.

`Ssh` struct'ına eklenecek: `open_shell(cols, rows) -> Result<Channel<Msg>>`.
(Şu an `handle` alanı private + `#[allow(dead_code)]`; kabuk kanalı için
kullanılacak, `dead_code` kalkacak.)

### 3.2 Terminal emülasyonu (VT100 parser + ratatui widget)
Sunucudan gelen ham bayt akışını (ANSI kaçış dizileri) bir ekran ızgarasına
çevirmek gerek. Seçilen kütüphaneler:

- **`vt100`** crate (doy) — bayt akışını işleyip renkli hücre ızgarası +
  imleç durumu tutan VT100 parser.
- **`tui-term`** crate — `vt100::Screen`'i ratatui widget'ı olarak çizer
  (`PseudoTerminal`). Bize sadece russh kanalından gelen baytları
  `vt100::Parser::process(&bytes)`'e beslemek kalır.

> ⚠️ Sürüm uyumu: `tui-term` + `vt100`'ün `ratatui = 0.29` ile uyumlu
> sürümlerini `cargo add` sırasında doğrula. Uymazsa alternatif: `vt100`'ü
> doğrudan kullanıp basit bir çizici yazmak, ya da `wezterm-term`/`termwiz`.

### 3.3 Klavye girişi → ANSI bayt kodlaması
crossterm `KeyEvent`'lerini kabuğa gidecek baytlara çeviren
`encode_key(KeyEvent) -> Vec<u8>` fonksiyonu:

| Tuş | Gönderilen |
|-----|-----------|
| Yazılabilir karakter | UTF-8 baytları |
| Ctrl+harf | `c & 0x1f` (ör. Ctrl+C = 0x03) |
| Enter | `\r` |
| Backspace | `0x7f` |
| Tab | `\t` |
| Esc | `\x1b` |
| ↑ ↓ → ← | `\x1b[A` `\x1b[B` `\x1b[C` `\x1b[D` |
| Home/End/PgUp/PgDn/Ins/Del | ilgili `\x1b[...~` dizileri |
| F3–F12 | ilgili kaçış dizileri |

> **Önemli**: F1 ve F2 **uygulama-global mod tuşlarıdır**, kabuğa
> iletilmezler. (İstenirse ileride mod tuşları `Ctrl+F1/F2`'ye taşınabilir.)

### 3.4 Async akış (kilitlenmez UI)
Terminal modunun döngüsünde `tokio::select!` ile üç kaynak dinlenir:
1. `channel.wait()` → sunucudan veri → parser'a besle → yeniden çiz.
2. crossterm `EventStream` → tuş → `encode_key` → kanala yaz; Resize → parser
   boyutu + `window_change`.
3. (İsteğe bağlı) global F1/F2 mod değişimi.

### 3.5 Uygulama seviyesinde mod yönetimi (F1/F2)
- Yeni bir üst kavram: `Screen { FileTransfer, Terminal }`.
- Bağlantı kurulduktan sonra ana döngü hangi ekranın aktif olduğuna göre
  ilgili alt-döngüyü/çizimi seçer.
- F1 → Terminal, F2 → FileTransfer. Geçişte bağlantı ve kabuk kanalı korunur.
- Üstte ince bir **durum/başlık çubuğu**: sunucu adı + aktif mod +
  `F1 Terminal | F2 Dosya | q Çıkış` ipucu.

---

## 4. Dosya bazında değişiklik haritası

| Dosya | Değişiklik |
|-------|-----------|
| `Cargo.toml` | `vt100`, `tui-term` bağımlılıkları (sürüm uyumu doğrula) |
| `src/ssh.rs` | `open_shell(cols, rows)` + kabuk kanalı yardımcıları; `handle` public erişimi |
| `src/terminal.rs` (**yeni**) | Terminal modu: state (vt100 parser, channel), döngü, çizim, `encode_key` |
| `src/main.rs` | `Screen` mod enum'u; F1/F2 anahtarlama; üst döngüde mod seçimi |
| `src/ui.rs` | Üst başlık/mod çubuğu; (dosya modu çizimi büyük ölçüde aynı) |
| `README.md` | F1/F2 kısayolları ve terminal modu kullanımını belgele |

---

## 5. Aşamalar (Durum)

### Aşama 0 — Hazırlık
- [x] `tui-term` + `vt100` sürüm uyumu çözüldü (aşağıdaki nota bak), `cargo add`.
- [x] Boş `terminal.rs` iskeleti + `mod terminal;` → yeşil derleme.

### Aşama 1 — Kabuk kanalı (SSH katmanı)
- [x] `Ssh::open_shell(cols, rows)` → pty + shell isteyen kanal döndürüyor (`ssh.rs`).

### Aşama 2 — Terminal emülasyonu + çizim
- [x] `vt100::Parser` (`TermSession`), gelen baytlar `feed()` ile besleniyor.
- [x] `tui_term::widget::PseudoTerminal` ile çizim (`terminal::draw`).
- [x] Resize → `set_size` + `window_change` (`TermSession::resize`).

### Aşama 3 — Klavye girişi
- [x] `encode_key` (Ctrl, Alt, ok tuşları, F3–F12, Home/End/PgUp/PgDn/Del vb.).
- [ ] Gerçek sunucuda `vim`/`htop` ile elle doğrula (kullanıcı testi bekliyor).

### Aşama 4 — Mod anahtarlama (F1/F2)
- [x] `Screen` enum + ana döngü dallanması (`main.rs run`).
- [x] F1/F2 global yakalama (kabuğa iletilmiyor).
- [x] Üst mod/başlık çubuğu (`terminal::draw_top_bar`, her iki modda ortak).
- [x] Tek SSH bağlantısı, tembel açılan kabuk kanalı modlar arası canlı.
- [x] Kabuk kapanınca (`exit`) dosya moduna otomatik dönüş.

### Aşama 5 — Cila (modern his) — TAMAMLANDI (2026-07-31)
- [x] 256/truecolor (vt100 + tui-term otomatik); başlık çubuğu stilize.
- [x] Kaydırma tamponu 1000 satır (`SCROLLBACK`) + tekerlek/Shift+PgUp ile gezinme.
- [x] Kabuk koparsa nazik dönüş + durum mesajı.
- [x] README güncellendi.
- [x] **Panoya yapıştırma** — `arboard`; sağ tık / Shift+Ins / Ctrl+Shift+V.
- [x] **Fareyle metin seçme / kopyalama** — mouse capture'ı kapatmaya gerek
      kalmadı: seçimi kendimiz çiziyoruz (`highlight_selection`) ve metni
      `vt100::Screen::contents_between` ile alıyoruz.
- [x] Büyük çıktıda `try_recv` ile batch besleme (daha az redraw).

> Uygulama notu: seçim ve kopyalama `visible_rows()` üzerinden çalıştığı için
> kaydırma tamponundayken de doğru metni verir (tui-term `screen.cell()` ile
> aynı görünümü çizer). Blok (dikdörtgen) seçim yok, seçim satır bazlı.

---

## 5.1 Sürüm uyumu — ÇÖZÜLDÜ (önemli not)
`tui-term 0.3.x` → `vt100 0.16.2` → `unicode-width ^0.2.1` ister; bu, `ratatui
0.29`'un `unicode-width =0.2.0` sabitiyle **çakışır**. Çözüm: `tui-term = "=0.2.0"`
+ `vt100 = "=0.15.2"` (bunlar `unicode-width 0.1` kullanır, ratatui'nin 0.2.0'ı
ile yan yana derlenir). Cargo.toml'da bu sürümler sabitlendi; yükseltirken dikkat.

## 6. Riskler / açık sorular
- **Sürüm uyumu**: ÇÖZÜLDÜ (bkz. 5.1). `tui-term`/`vt100` sürümlerini sabit tut.
- **Fare**: Terminal modunda fare şu an mod çubuğu için; uzak programlara fare
  iletimi (SGR mouse) ileri seviye, ilk sürümde kapsam dışı.
- **F1/F2 çakışması**: Uzaktaki programlar F1/F2 bekliyorsa çakışır; şimdilik
  mod tuşları olarak ayrılıyor (gerekirse Ctrl+F1/F2'ye taşı).
- **Windows kripto**: `russh` `ring` backend'i zaten seçili (NASM gerekmez) —
  yeni bağımlılık eklerken bozma.

---

## 7. Sıradaki adım
> Aşama 5 bitti; çalışma ağacında **commit'lenmemiş** değişiklikler var
> (`Cargo.toml`, `Cargo.lock`, `main.rs`, `terminal.rs`, `ui.rs`).
>
> 1. ✅ Elle test edildi: seçme/kopyalama, sağ tık yapıştırma, Ctrl+V & arkadaşları.
> 2. **v0.2.0 yayını** — komutlar "## 12"de; kullanıcı çalıştırıyor.
> 3. **Linux sürümü** (bkz. "## 11") — `arboard` yeni bağımlılık, orada doğrula.
>
> Henüz elle denenmemiş olanlar (fırsat oldukça): `vim`/`htop` tam ekran,
> tekerlekle geçmişe kaydırma, `Shift+PgUp/PgDn`.

## 8. Bağlam (kod referansları)
- Kabuk kanalı: `src/ssh.rs` — `Ssh::open_shell(cols, rows)` aynı bağlantıda
  ikinci kanalı açıp pty+shell istiyor.
- Terminal modülü: `src/terminal.rs` — `TermSession` (writer + vt100 parser +
  arka plan okuma task'i), `encode_key`, `draw`, `draw_top_bar`.
- Ana döngü: `src/main.rs` — `Screen` enum + `run(...)` içinde `tokio::select!`
  (klavye/fare ↔ kabuk çıktısı). Borrow çakışmasını önlemek için oturum aç/kapa
  `pending_open`/`shell_closed` bayraklarıyla döngü başında (select dışında)
  yapılıyor — bu deseni bozma.
- Üst çubuk: `terminal::draw_top_bar` hem `ui::draw` (dosya) hem `terminal::draw`
  tarafından kullanılıyor.
- Tuş çift-algılama: tüm giriş noktalarında `KeyEventKind::Press` filtresi var.

---

## 9. Yarınki plan (2026-07-25)

### 9.A GitHub'a atma
Depo henüz git değil. `gh` mevcut (2.93.0). `.gitignore` zaten `/target` ve
`config.json`'ı yok sayıyor — **parolalar güvende**, ama push öncesi son kontrol şart.

Adımlar:
1. **Güvenlik kontrolü (ÖNCE)**: `config.json` gerçek parolalar içeriyor. Push
   etmeden önce `git status`/`git check-ignore config.json` ile yok sayıldığını
   doğrula. Başka gizli dosya (ör. `sunucular.json` gibi alternatif config'ler)
   varsa onları da `.gitignore`'a ekle. `config.example.json` şablon olarak kalsın.
2. `git init` → `git add .` → durumu gözden geçir (config.json listede OLMAMALI).
3. İlk commit (mesaj sonuna Co-Authored-By satırı — repo kuralı).
4. `gh repo create <ad> --private --source . --remote origin --push`
   (ad ör. `sftp-tui` ya da `ssh-file-manager`; **private** öneriyorum — içerik
   sunucu yönetimi aracı). Kullanıcıya isim/gizlilik sor.
5. Push sonrası GitHub'da config.json'ın görünmediğini teyit et.

> NOT: Commit/push yalnızca kullanıcı istediğinde. Yarın kullanıcı onayıyla yapılacak.

### 9.B BUG — tek satır komut sonrası ekran temizleniyor ✅ ÇÖZÜLDÜ
**Çözüm (2026-07-25)**: `terminal.rs::scan_queries` — gelen akışta CPR/DSR/DA
sorguları taranıp yanıtlanıyor; `feed()` yanıtı biriktiriyor, ana döngü
`take_reply()` ile kabuğa gönderiyor. Aşağıdaki eski analiz referans için duruyor.
Kullanıcı gerçek sunucuda hâlâ görürse `TFS_LOG` ile ham akışı yakala.

<details><summary>Eski analiz (referans)</summary>


**Belirti**: Terminal (F1) güzel çalışıyor; tek satır komut yazıp Enter'a basınca
komut **işleniyor**, ama ~1 saniye sonra **ekran temizleniyor**.

**Tanı (önce bunu yap — kesin yol)**: Sunucudan gelen ham bayt akışını geçici
olarak bir dosyaya logla. `terminal.rs`'te okuma task'inde `tx.send`'den hemen
önce `data`'yı escape'leyip (ör. `{:?}`) bir dosyaya/`eprintln`'e yaz. Reprodüksiyon
sonrası Enter'dan ~1 sn sonra gelen diziye bak. Şunları ara:
- `ESC[2J` (ekranı temizle), `ESC[3J` (scrollback temizle), `ESC[H` (home),
- `ESC[?1049h` / `ESC[?1049l` (alternatif ekran gir/çık),
- `ESC[J`, `ESC[K` (satır/ekran sonu temizle).
Bu, temizliğin **sunucudan mı geldiğini** (shell/prompt yapılandırması ya da
bizim yanlış boyut/`TERM` bildirimimiz) yoksa **bizim çizimimizin mi** içeriği
düşürdüğünü ayırt eder.

**Hipotezler (olasılık sırasına göre)**:
1. **`window_change`/SIGWINCH reflow (en güçlü aday)**: Çizim döngüsünde her
   karede `resize()` çağrılıyor; boyut değişmezse no-op ama `terminal.size()`
   Windows'ta render sırasında anlık oynayabilir. Değişince `window_change` →
   sunucuda SIGWINCH → shell/prompt (zsh-zle, bash-readline, starship/p10k)
   satırı/ekranı yeniden çiziyor. **Asenkron promptlar (starship git durumu vb.)
   ~1 sn sonra tekrar boyayıp temizleyebilir** — "1 saniye" gecikmesini bu açıklar.
   → Düzeltme: resize'ı her karede değil, yalnızca gerçek `Event::Resize`'da yap.
     Boyutu bir kez açılışta ayarla; poll etme.
2. **`vt100 set_size` içeriği siliyor olabilir**: Boyut spuriously değişirse
   `parser.set_size` grid'i reflow/temizleyebilir. (1) ile birleşince tam oturuyor.
   → set_size'ı sadece gerçek değişimde çağırdığımızdan emin ol + logla.
3. **Prompt yapılandırması**: Kullanıcının `PROMPT_COMMAND`/prompt teması ekran
   temizliyor olabilir (ör. `clear` benzeri OSC). Log bunu gösterir; bizde
   düzeltme gerekmeyebilir ama `TERM`/boyut doğruluğuyla tetiklenmemeli.
4. **Enter kodlaması**: `\r` gönderiyoruz (doğru). Düşük olasılık; log net değilse
   `\r` yerine davranışı gözden geçir.

**Önerilen ilk müdahale**: resize mantığını "her karede poll" yerine crossterm
`Event::Resize` olayına bağla (main.rs terminal kolunda). Sonra tekrar test et;
düzelmezse ham log'a bak.

</details>

### 9.C Benim ek önerilerim (öncelikli sıra)
- ✅ **Yapıştırma** — bracketed paste ile eklendi (arboard'a gerek kalmadı).
- ✅ **Büyük çıktı performansı** — döngü başında `try_recv` batch besleme eklendi.
- ✅ **Fareyle sekme geçişi** — üst çubuktaki F1/F2 sekmeleri tıklanabilir
  (`terminal::hit_tab`, main.rs global mouse yakalama).
- ✅ **Fareyle metin seçme/kopyalama** + panodan yapıştırma + kaydırma (2026-07-31).
- [ ] **Güvenlik (eski iskelet borçları)**: `known_hosts` doğrulaması
  (`check_server_key` şu an daima `true`) + publickey (anahtar) auth.
- **Klasör (recursive) transferi** (F2 tarafı — hâlâ sadece tek dosya).
- **Bağlantı kopunca** nazik yeniden bağlanma / picker'a dönüş.

### 9.D (arşiv) İlk gün başlangıç notu — artık geçildi
> Bug çözüldü (9.B) ve GitHub'a atıldı (bkz. üst özet). Bu madde tarihsel.

---

## 10. Derleme & Dağıtım (2026-07-25)

### 10.1 Windows binary'leri
- **x86_64 (64-bit)**: `cargo build --release` → `target/release/tfs.exe` (~4.7 MB). ✅
- **i686 (32-bit)**: `rustup target add i686-pc-windows-msvc` →
  `cargo build --release --target i686-pc-windows-msvc` →
  `target/i686-pc-windows-msvc/release/tfs.exe` (~3.8 MB). ✅ (ring/russh sorunsuz derlendi.)
- Release asset adları: `dist/tfs-v<sürüm>-windows-{x86_64,i686}.exe`. `dist/` gitignore'lu.

### 10.2 Windows 7/8 — DESTEKLENMİYOR (bilinçli)
Rust 1.78+ standart `*-pc-windows-msvc` hedefi Win7/8'i bıraktı; binary Win10+ ister.
Tek yol Tier-3 `x86_64-win7-windows-msvc` + nightly + `-Z build-std` (deneysel,
test edilmedi). Kullanıcı kriteri "basitse yap" → yapılmadı. Tarif (istenirse):
```
rustup toolchain install nightly
rustup component add rust-src --toolchain nightly
cargo +nightly build --release -Z build-std --target x86_64-win7-windows-msvc
```

### 10.3 GitHub Release
- `gh release create v0.1.0` ile iki `.exe` yüklendi. Repo: okanaytimur/tfs.

## 11. SONRAKİ AŞAMA — Linux sürümü (NOT)
> Kullanıcı, tüm dosyalarla birlikte bir **Linux makinada** derleme/çalıştırma
> yaptırmak isteyecek. Beklenen: kod zaten taşınabilir (ratatui/russh/crossterm
> Linux'ta çalışır). Yapılacaklar:
> - `cargo build --release` (Linux host) → ELF binary; mouse/paste/terminal
>   crossterm ile Linux'ta da çalışır.
> - Kripto backend: `ring` Linux'ta NASM istemez, sorun beklenmez (Windows için
>   seçilmişti; Linux'ta da uyumlu).
> - Release'e Linux binary (`tfs-vX-linux-x86_64`) + belki `.tar.gz` eklenebilir.
> - Cross-compile yerine gerçek Linux'ta derlemek en temizi (glibc uyumu için
>   mümkünse eski bir dağıtım ya da `x86_64-unknown-linux-musl` ile statik binary).
> - CI (GitHub Actions) ile Win+Linux otomatik release ileride düşünülebilir.
>
> **DİKKAT — `arboard` (2026-07-31 eklendi)**: Linux'ta pano için `x11rb` (saf Rust,
> sistem kütüphanesi istemez) + `wayland-data-control` özelliğiyle `wl-clipboard-rs`
> kullanılıyor. Wayland yolu `wayland-client`'ın **saf Rust** backend'ini seçtiği
> için `libwayland-dev` gerekmemeli — ama Linux'ta ilk derlemede DOĞRULA.
> Derleme patlarsa çözüm: `Cargo.toml`'da `features = ["wayland-data-control"]`
> listesini boşalt (X11/XWayland ile pano yine çalışır). Pano hiç açılamazsa kod
> zaten nazikçe düşüyor (`clipboard: None` → üst çubukta "⚠ pano yok").
>
> **NOT — yapıştırma Linux'ta farklı çalışır**: Linux terminalleri bracketed
> paste'i destekler, yani crossterm gerçek `Event::Paste` üretir ve yığın toplama
> (`collect_key_burst`) hiç devreye girmez. Windows'a özgü bir çözümdü; Linux'ta
> yapıştırmanın zaten tek parça geldiğini doğrula.

---

## 12. v0.2.0 yayın komutları (2026-08-01)

Sürüm `Cargo.toml`'da 0.2.0'a çıkarıldı, `cargo package` doğrulandı. Release
notları: `dist/RELEASE-v0.2.0.md` (dist gitignore'lu). Komutlar kullanıcıda.

```powershell
# Çalışan tfs.exe varsa kapat (target kilitlenmesin)
cargo clippy --all-targets; cargo test; cargo build --release
git add -A; git commit; git tag v0.2.0
git push origin main; git push origin v0.2.0
cargo build --release --target i686-pc-windows-msvc
gh release create v0.2.0 <exe'ler> --notes-file dist\RELEASE-v0.2.0.md
cargo publish --dry-run; cargo publish
```

> `cargo publish` geri alınamaz (yalnızca `yank`). Önce `--dry-run`.

Sonraki sürümlerde: `Cargo.toml`'da `version` artır → aynı adımlar.
