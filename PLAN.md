# PLAN — F1: SSH Terminali + F2: Dosya Transferi

> Bu dosya **oturumlar arası devam** içindir. Yeni bir oturum açtığında önce
> bunu oku; "Durum" bölümündeki kutucuklardan (`[ ]` / `[x]`) nerede kaldığımızı
> gör ve "Sıradaki adım"dan devam et.

Son güncelleme: 2026-08-31

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
> **2026-08-30 (Aşama 6 — fresh editör entegrasyonu):**
> - ✅ **F4 / `e` → dosyayı [`fresh`](https://github.com/sinelaw/fresh) ile düzenle**
>   (yeni modül `src/editor.rs`, ~430 satır + testler).
>   - YEREL panel: dosya olduğu yerde açılır.
>   - UZAK panel: geçici dizine indir → editörde aç → **içerik değiştiyse**
>     SFTP ile geri yükle. Uzak sunucuda editör kurulu olmasına gerek yok.
>   - Değişiklik tespiti **içerik özetiyle** (FNV-1a 64-bit, akış hâlinde),
>     zaman damgasıyla değil — editör kaydetmeden kapansa da yükleme olmaz.
>   - İndirme/yükleme mevcut progress bar'ı kullanır (`run_transfer` artık
>     `Result<bool>` döndürüyor; çağıran başarıyı bilmeli).
>   - Yükleme başarısızsa geçici dosya **bilerek silinmez** — kullanıcının emeği
>     orada; yol durum çubuğunda yazar.
>   - Uzak dosya sınırı 64 MiB (`editor::MAX_EDIT_BYTES`).
> - ✅ **Kurulum: `cargo binstall`**. `fresh` yoksa TUI'de onay kutusu çıkar;
>   onaylanırsa sıra: `cargo binstall --no-confirm fresh-editor` → yoksa önce
>   `cargo install cargo-binstall` → son çare `cargo install --locked
>   fresh-editor`. Çıktı düz terminalde akar. crates.io paketi **`fresh-editor`**,
>   binary **`fresh`** (0.4.10, GPL-2.0). Yeni Rust bağımlılığı YOK.
> - ⚠️ **Kritik ayrıntı — TUI askıya alma**: editör çalışırken crossterm
>   `EventStream`'i **düşürülmeli**. `poll_next` Pending dönünce crossterm arka
>   planda bir thread'i tty üzerinde bloklayan okumaya sokuyor
>   (`event/stream.rs`); bu thread ayakta kalırsa kullanıcının tuşları editöre
>   değil bize gelir.
> - 🐞 **DEADLOCK — ilk deneme böyle patladı, tekrar yapma.** `editor::suspend`
>   önce `mem::replace(events, EventStream::new().peekable())` yapıyordu: yeni
>   akış **eskisi düşürülmeden önce** kuruluyor. Ama:
>   - `EventStream::default()` kurulurken `lock_internal_event_reader()` ile
>     **global okuyucu mutex'ini** alır (`stream.rs:64`),
>   - yoklanmış bir akışın arka plan thread'i ise aynı mutex'i
>     `poll_internal(None, …)` içinde **bloklayan `poll` boyunca elde tutar**
>     (`event.rs:256-270` — zaman aşımı yoksa `lock_internal_event_reader()`).
>
>   Yani yeni akış, eski akışın thread'inin tuttuğu kilidi bekler; o thread'i
>   uyandıracak olan `Drop` ise henüz çalışmamıştır → **kilitlenme**.
>   Belirti: *yerel* F4 çalışıyor (tuş geldiği an thread kilidi bırakmış olur),
>   *uzak* F4'te tfs "kapanıyor" gibi görünüp asılıyor — çünkü araya giren
>   `run_transfer`'ın `select!`'i `events`'i yoklayıp thread'i yeniden
>   bloklatıyor, indirme başka koldan bitince o thread serbest kalmıyor;
>   `suspend` ise ilk iş `restore_terminal` yaptığı için ekran normale döndüğünden
>   uygulama kapanmış sanılıyor.
>
>   **Çözüm**: `main::EventSource` (`Option<Events>`) — `shutdown()` akışı
>   **önce düşürür**, `get()` ise ihtiyaç anında (alt süreç bittikten sonra)
>   tembel yeniden kurar. Sırayı bozma: önce düşür, sonra kur.
> - `main.rs`: `setup_terminal` → `enter_tui` olarak ayrıştırıldı (geri dönüşte
>   `terminal.clear()` şart, yoksa ratatui eski kareyi geçerli sanar).
> - 🔴 **DERLENMEDİ**: bu Fedora makinasında `gcc` yok (`error: linker \`cc\` not
>   found`). `sudo dnf install -y gcc` sonrası `cargo clippy --all-targets` +
>   `cargo test` + gerçek sunucuda F4 testi YAPILMALI.

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

### Aşama 6 — fresh editör entegrasyonu (2026-08-30)
- [x] `src/editor.rs`: `locate` (PATH + `~/.cargo/bin` + `~/.local/bin`),
      `suspend`/`resume`, `run_suspended`, `install`, `content_hash`,
      `temp_file_for`/`cleanup_temp`, `draw_install_prompt`.
- [x] `app.rs`: `EditRequest` + `pending_edit` + `request_edit`, F4/`e` tuşu.
- [x] `main.rs`: `run_edit` (yerel/uzak akışları), `confirm_install`,
      `enter_tui`, `run_transfer` → `Result<bool>`.
- [x] README: "F4 — dosyayı `fresh` ile düzenle" bölümü.
- [x] Linux'ta derleme + clippy temiz + `cargo test` 16/16 (2026-08-30, gcc kuruldu).
- [x] `main::EventSource` — askıya alma kilitlenmesi düzeltildi (bkz. üstteki 🐞).
- [x] Kullanıcı testi: **YEREL** F4 çalışıyor.
- [ ] Kullanıcı testi: **UZAK** F4 (deadlock düzeltmesinden sonra tekrar).
- [ ] Kaydetmeden çık → yükleme olmamalı. Kurulum akışı (`PATH=` ile dene).
- [x] 🐞 `temp_file_for` aynı milisaniyede çakışıyordu (pid+ms yetmiyor) →
      dizin artık münhasıran açılıyor (`create_dir` + sayaç), test eklendi.

### Aşama 8 — klasör (recursive) transferi (2026-08-30)
- [x] `ssh.rs`: `walk_local` / `Ssh::walk_remote` (genişlik-öncelikli; **dizin
      daima içeriğinden önce** listeye girer — hedefte dizinler içerik gelmeden
      açılsın diye. Test: `dizinler_iceriklerinden_once_gelir`).
- [x] `Ssh::upload`/`download` artık ağaç aktarıyor (tek dosya = tek kalemlik
      ağaç, davranış aynı), `TreeOutcome` döndürüyor.
- [x] Sembolik bağlar **izlenmiyor** (dizin bağı = sonsuz döngü), sayılıp
      bildiriliyor. Listelenemeyen dizinler de atlanıp sayılıyor.
- [x] Tek dosya hatası transferi durdurmuyor; `TreeOutcome.first_error` durum
      çubuğuna düşüyor.
- [x] İlerleme: önce tarama (gerçek toplam), sonra dosya sayacı + o anki dosya
      yolu (`TransferProgress.label` / `.files`). `Reporter` çubuğu **monoton**
      tutuyor (dosya bitiminde beklenen boyuta sabitleniyor).
- [x] `ui.rs`: ilerleme kutusu 4 satır — başlıkta dosya sayacı, altta o anki yol.
- [x] 7 yeni test (toplam 23), clippy temiz.
- [x] Kullanıcı doğruladı: iç içe klasör her iki yönde çalışıyor (2026-08-30).

### Aşama 7 — klavyeyle transfer (2026-08-30)
- [x] **`t` / F5** → odaklı paneldeki seçili dosyayı karşı panele aktar.
      `App::request_transfer_selected`; sürükle-bırakla ortak `queue_transfer`
      (eski `request_transfer` artık onun ince bir sarmalayıcısı).
- [x] Kullanıcı doğruladı (2026-08-30).

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
> **v0.4.0 GitHub'da yayında** (2026-08-31): push + release tamam, Linux binary
> asset olarak yüklü. Kalanlar "## 12"de:
>
> 1. ~~`git push origin main` + `git push origin v0.4.0`~~ ✅
> 2. ~~`gh release create v0.4.0 …`~~ ✅
> 3. [ ] `cargo login` → `cargo publish --dry-run` → `cargo publish`
>        (crates.io'da hâlâ 0.2.0 duruyor; bu makinada token yok).
> 4. [ ] Windows makinada iki `.exe`yi derleyip `gh release upload` ile ekle.
>
> Sonraki iş (kod): "## 9.C" — `known_hosts` doğrulaması + publickey auth
> (güvenlik borcu), klasör (recursive) transferi, bağlantı kopunca yeniden
> bağlanma.
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
- Editör: `src/editor.rs` (mekanik) + `main.rs::run_edit` (akış). Askıya alma
  deseni için üstteki 2026-08-30 notundaki `EventStream` uyarısını oku.

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
- ✅ **Klasör (recursive) transferi** — Aşama 8'de eklendi (2026-08-30).
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
> ✅ **2026-08-30 DOĞRULANDI (Fedora 44, rustc 1.97.1)**: `cargo build --release`
> sorunsuz; binary 7.5 MB ELF. `ldd`: yalnızca `libc`/`libm`/`libgcc_s` —
> **X11/Wayland sistem kütüphanesi gerekmedi**, yani aşağıdaki `arboard` endişesi
> geçersiz çıktı (`wayland-data-control` özelliği kalabilir). Tek ön koşul: C
> linker (`gcc`) — Fedora'da `dnf install gcc`.
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

## 12. Yayın adımları

> `cargo publish` geri alınamaz (yalnızca `yank`). Önce `--dry-run`.
> Sonraki sürümlerde: `Cargo.toml`'da `version` artır → aynı adımlar.
>
> **Yayın sonrası doğrulama** (kimlik gerektirmez, herkesin makinasında çalışır):
> ```sh
> # crates.io: hangi sürüm, yanked mı, hangi commit'ten
> curl -s https://crates.io/api/v1/crates/tfs-ssh | python3 -m json.tool | head -30
> curl -sL https://static.crates.io/crates/tfs-ssh/tfs-ssh-X.Y.Z.crate -o /tmp/p.crate
> tar xzOf /tmp/p.crate tfs-ssh-X.Y.Z/.cargo_vcs_info.json
> # GitHub release + asset'ler
> curl -s https://api.github.com/repos/okanaytimur/tfs/releases/tags/vX.Y.Z
> # binstall URL'leri gerçekten var mı (yapılandırma "doğru görünmek" yetmez)
> curl -sIL -o /dev/null -w '%{http_code}\n' \
>   https://github.com/okanaytimur/tfs/releases/download/vX.Y.Z/tfs-vX.Y.Z-linux-x86_64
> # etiket ile yayınlanan commit aynı mı
> git ls-remote --tags origin | grep 'vX.Y.Z'
> ```

### v0.4.0 — YAYINDA (GitHub release + crates.io, 2026-08-31)

> ⚠️ **Neden 0.3.0 atlandı**: `v0.3.0` etiketi `9f32034`'e basıldı ve push
> edildi, ama klasör desteği (`44250a1`) ondan **sonra** geldi. 0.3.0 hiçbir
> yere yayınlanmadığı (crates.io'da en son 0.2.0) ve itilmiş etiketi taşımak
> istemediğimiz için sürüm 0.4.0'a çıkarıldı. `v0.3.0` release'siz bir ara
> durak olarak kalıyor.

Yapıldı:
- [x] Sürüm 0.4.0. `exclude = ["PLAN.md"]` — bu dosya 33 KB'lık iç çalışma notu,
      crates.io paketinin ~yarısıydı ve kullanıcıya hitap etmiyordu (80K → 67K).
- [x] `cargo clippy --all-targets` temiz · `cargo test` 23/23 · release derlendi.
- [x] `cargo package` doğrulandı (`config.example.json` pakette, PLAN.md yok).
- [x] `dist/tfs-v0.4.0-linux-x86_64` (5.5 MB, strip'li) — asgari **glibc 2.34**
      (objdump'taki GLIBC_2.39 sembolleri *zayıf*, sorun değil).
- [x] `dist/RELEASE-v0.4.0.md` — 0.2.0'dan bu yana her şey.
- [x] Commit + `v0.4.0` etiketi.
- [x] Depo yapısı denetlendi: 16 takip edilen dosya, geçmişte `config.json` /
      anahtar / parola sızıntısı **yok**.

Yayınlandı (2026-08-31, `gh` 2.97.0 kuruldu):
- [x] `git push origin main` + `git push origin v0.4.0` → `01dc454` ve `v0.4.0`
      etiketi origin'de.
- [x] GitHub Release: https://github.com/okanaytimur/tfs/releases/tag/v0.4.0
      (asset: `tfs-v0.4.0-linux-x86_64`, 5.697.192 bayt; notlar
      `dist/RELEASE-v0.4.0.md`'den).

- [x] Windows binary'leri Windows makinada derlenip release'e yüklendi
      (`tfs-v0.4.0-windows-x86_64.exe` 5.264.896 B ·
      `tfs-v0.4.0-windows-i686.exe` 4.297.216 B).
- [x] `cargo update -p chacha20` (0.10.1 **yanked** → 0.10.2). russh'un dolaylı
      bağımlılığı; `cargo publish` uyarı veriyordu. 23/23 test geçti. Bu yüzden
      yayınlanan crate'in Cargo.lock'u `v0.4.0` etiketinden bir commit ileride —
      release binary'leri etiketten derlendiği için etkilenmedi.

- [x] `[package.metadata.binstall]` eklendi → `cargo binstall tfs-ssh` hazır
      binary indirir. **Bu bölüm crates.io'ya yüklenen sürümden okunur**, GitHub
      deposundakinden değil — yayınlanmış bir sürüme sonradan eklenemez, o yüzden
      yayından ÖNCE girdi. Varsayılan kalıplar Rust hedef üçlüsü beklediği için
      (`tfs-ssh-x86_64-pc-windows-msvc-v0.4.0.exe`) hedef başına `overrides`
      yazıldı; mevcut asset adları korundu, üç URL de 200 döndü. README'ye
      `cargo binstall` satırı eklendi.

- [x] **crates.io: `tfs-ssh 0.4.0` yayında** (2026-08-31 11:04:36 UTC, Windows
      makinasından). Yanked değil. Yayınlanan paketin `.cargo_vcs_info.json`'u
      `64a5269` diyor — yani chacha20 düzeltmesi ve binstall metadatası dahil.
      (Not: `--dry-run` yayın değildir; bir tur sadece o çalıştırıldığı için
      0.4.0 bir süre crates.io'da görünmedi.)
- [x] **Etiket düzeltildi** (2026-08-31): `v0.4.0` `01dc454`'i gösteriyordu ama
      crates.io'ya `64a5269` yayınlanmıştı — etiketten derleyen biri farklı
      kaynak alırdı. Etiket `64a5269`'a taşındı ve force-push edildi
      (`git tag -f -a v0.4.0 64a5269` + `git push --force origin v0.4.0`).
      GitHub release ve asset'leri etkilenmedi (indirme sayaçları korundu).

> 📌 **Ders — sonraki sürümlerde etiketi EN SON bas.** İki kez üst üste aynı şey
> oldu (v0.3.0 ve v0.4.0): etiket basıldı, sonra commit'ler geldi, etiket geride
> kaldı. Doğru sıra:
> 1. Tüm değişiklikleri commit'le (`Cargo.lock` yanked düzeltmeleri,
>    `metadata.binstall` gibi meta işleri dahil).
> 2. `cargo publish --dry-run` → sorun varsa düzelt, **tekrar commit'le**.
> 3. **Şimdi** etiketle: `git tag -a vX.Y.Z`.
> 4. `git push origin main && git push origin vX.Y.Z`.
> 5. `cargo publish` + `gh release create` — ikisi de etiketli commit'ten.
Windows binary'leri **Windows makinada** derlenmeli, sonra release'e eklenmeli:
```powershell
cargo build --release                                  # x86_64
cargo build --release --target i686-pc-windows-msvc    # i686
```
```sh
gh release upload v0.4.0 dist/tfs-v0.4.0-windows-x86_64.exe dist/tfs-v0.4.0-windows-i686.exe
```

### İsteğe bağlı — musl statik Linux binary'si
glibc 2.34 altındaki dağıtımları (Ubuntu 20.04, Debian 11, CentOS 7) da
kapsamak için. `ring`'in C kodu musl derleyicisi ister:
```sh
sudo dnf install -y musl-gcc          # Fedora paketi mevcut (1.2.5)
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
```

### v0.2.0 (2026-08-01) — yayınlandı
crates.io + GitHub Releases (Windows x86_64 + i686).
