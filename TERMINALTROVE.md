# Terminal Trove — gönderim metni

> Kullanıcı dokümanı **değil**: [terminaltrove.com/submit](https://terminaltrove.com/submit)
> formuna yapıştırılacak metin burada duruyor ki her seferinde yeniden yazılmasın.
> Başlıklar formdaki alan adlarıyla birebir aynı sırada. Karakter sayıları
> `assets/../TERMINALTROVE.md` üretilirken doğrulandı (form sınırlarına uyuyor).
>
> Yeni sürümde: README'nin "English" bölümüyle birlikte güncelle.

## Önce kriterler

| # | Kriter | Durum |
|---|--------|-------|
| 1 | Ideally cross platform | ⚠️ Linux + Windows var, **macOS yok** — "ideally" dediği için engel değil |
| 2 | Standalone binaries preferred | ✅ Linux statik (musl) + Windows x64/x86 |
| 3 | **Must have an image preview (PNG, GIF veya MP4)** | ❌ **PNG hâlâ gerekli** — aşağı bak |
| 4 | Must not exist already | ⬜ [Tam listeyi](https://terminaltrove.com/list/) kontrol et, sonra kutuyu işaretle |

> ⚠️ **En kritik eksik: PNG.** Form PNG'yi *zorunlu*, GIF'i *önerilen* tutuyor.
> Yani planladığınız GIF tek başına yetmez, bir de durağan ekran görüntüsü lazım.
> Örneklerdeki araçların çoğunda (rustscan, tortuise, dolphie) ikisi birden var.

---

## Basic Info

**name**
```
tfs
```

**url** (form "no https://" diyor)
```
github.com/okanaytimur/tfs
```

**tagline** (78/100 karakter)
```
Two-pane SFTP file transfer and a real SSH terminal, over a single connection.
```

**source code** (opsiyonel — dil ve lisansı buradan otomatik doldurur)
```
https://github.com/okanaytimur/tfs
```

---

## Description

**describe your tool.** (312 kr · 100-400)
```
tfs puts a two-pane SFTP file manager and a full interactive SSH terminal on one SSH connection. Press F2 to drag files and folders between local and remote with the mouse; press F1 for a real VT100 terminal where vim and htop work. Switching between them never reconnects and never asks for your password again.
```

**2-3 standout features** (347 kr · 60-400)
```
- F2: two-pane local/remote file manager. Drag files and whole folders across with the mouse, with live progress and cancel.
- F1: a real VT100 terminal (vim, htop, less), with mouse text selection, clipboard copy/paste and scrollback.
- A built-in connection manager: add, edit, duplicate, reorder and search your servers without leaving the TUI.
```

**what makes this different?** (396 kr · 50-500)
```
Most setups make you run two tools - a file manager and a terminal - each with its own login. tfs opens both as channels on a single SSH connection, so switching is instant and nothing re-authenticates. And because it speaks SSH itself (russh) instead of shelling out to the system ssh client, it can optionally store a password per host: masked on screen, written atomically, chmod 0600 on Unix.
```

**other notable features** (364 kr · opsiyonel, max 400)
```
F4 opens the selected remote file in the fresh editor and uploads it back when you save. Host keys are checked against the same ~/.ssh/known_hosts OpenSSH uses, and publickey auth is tried before password, in OpenSSH's order. Start typing in any pane to filter it - there is no separate search mode. The Linux binary is statically linked, so it runs on any distro.
```

**who is this for / when to use it** (222 kr · 60-300)
```
For anyone who moves files to a remote box and keeps a shell open beside it: sysadmins, developers deploying over SFTP, homelab users. Reach for it when running WinSCP next to PuTTY starts feeling like one window too many.
```

**anything else we should know?** (opsiyonel, yayınlanmıyor)
```
Dual-licensed MIT OR Apache-2.0 - the form takes one, so MIT is selected. Linux (x86_64, static musl binary) and Windows 10+ (x86_64 and i686) are tested; macOS is untested because I have no Mac - the dependencies are cross-platform, so it may well build. The README is Turkish with a full English section.
```

---

## Technical Details

| Alan | Seçim |
|------|-------|
| primary language | `rust` |
| license | `mit` |

> Proje **MIT OR Apache-2.0** ikili lisanslı ama form tek seçim istiyor; `mit`
> seçilip durum "anything else" alanında açıklandı.

---

## Image Preview

| Alan | Durum |
|------|-------|
| preview image (PNG) — **zorunlu** | ⬜ Hazırlanacak |
| preview image (GIF) — önerilen | ⬜ Hazırlanacak |

**PNG için**: bağlantı yöneticisi ekranı en iyi kareyi veriyor — liste + ayrıntı
paneli + araç çubuğu aynı anda görünüyor, aracın ne olduğu tek bakışta anlaşılıyor.
Alternatif: F2'de iki panel (YEREL ↔ UZAK) dolu hâlde.

**GIF için** 10-15 saniyelik sıra:

1. Bağlantı yöneticisinde `[+ Yeni]` → bir sunucu ekle
2. Satıra tıkla → bağlan
3. F2'de yerelden uzağa bir klasör sürükle (ilerleme çubuğu görünsün)
4. F1'e geç, `htop` ya da `vim` aç — aynı bağlantı, yeniden bağlanma yok
5. Panelde yazmaya başla, listenin filtrelendiğini göster

GIF'i `assets/demo.gif` olarak koyup README'nin başındaki yorum satırını aç.

---

## Categories

Formdaki listelerden geçerli olanlar:

| Grup | Seçilecek |
|------|-----------|
| All Tools | `tui`, `cli` |
| Networking & Security | `ssh`, `networking` |
| Files & Data Management | `file-transfer`, `file-manager` |
| Platforms | `linux`, `windows`, `ssh-apps` |
| TUI Frameworks | `ratatui` |

> `cross-platform` sizin kararınız: Linux + Windows var ama macOS/BSD yok.
> Zorlamamak daha dürüst, seçmemeyi öneririm.

---

## Install Instructions

Form paket yöneticisi başına ayrı satır istiyor ve "paket depoda gerçekten var mı"
diye uyarıyor — `tfs-ssh` crates.io'da yayında, doğrulandı.

| Platform | Paket yöneticisi | Komut |
|----------|------------------|-------|
| All | cargo | `cargo install tfs-ssh` |
| All | cargo-binstall | `cargo binstall tfs-ssh` |
| Linux | binary | `curl -LO https://github.com/okanaytimur/tfs/releases/latest/download/tfs-v0.6.0-linux-x86_64 && chmod +x tfs-v0.6.0-linux-x86_64` |
| Windows | binary | Releases sayfasından `tfs-v0.6.0-windows-x86_64.exe` |

> ⚠️ **"auto-fill tool name in commands" seçeneğini kapatın.** Araç adı `tfs`
> ama crates.io paketi `tfs-ssh` (`tfs` adı başkasında). Otomatik doldurma
> `cargo install tfs` yazar ve o komut **yanlış bir paketi kurar**.

---

## Author & Confirmation

| Alan | Değer |
|------|-------|
| are you the author? | `yes` |
| email | *(kendi e-postanız — "yes" seçilince zorunlu)* |
| criteria onayı | ⬜ Kutuyu işaretle |

---

## E-posta ile gönderme (alternatif)

Form yerine `curator [at] terminaltrove.com` adresine de yazılabiliyor. Konu:
`Suggestion for a tool on Terminal Trove: tfs` — gövdeye name / url / author /
email dörtlüsü yeterli, gerisini yukarıdaki metinlerden ekleyin.
