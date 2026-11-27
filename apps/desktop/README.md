# Routune masaüstü (Tauri 2)

Web istemcisinin (`apps/client`) üretim build'ini paketleyen masaüstü uygulaması. Kararlar: ADR-0020 (bearer oturum, Rust WebSocket) ve ADR-0023 (dağıtım ve güncellemeler).

- Oturum token'ı işletim sisteminin güvenli deposunda tutulur: macOS Keychain, Windows Credential Manager, Linux Secret Service (`session_token_*` komutları, `src-tauri/src/lib.rs`).
- `/ws` bağlantısı `tauri-plugin-websocket` ile Rust tarafından açılır.
- Uygulama `tauri://localhost` (macOS/Linux) ya da `http://tauri.localhost` (Windows) origin'inden çalışır. Sunucudaki `CORS_EXTRA_ORIGINS` bu iki adresi içermelidir.

## Yerelde derleme

```bash
EXPO_PUBLIC_API_URL=https://api.routune.dev pnpm --filter @secondbrain/desktop build
```

Çıktılar `src-tauri/target/release/bundle/` altına yazılır. `pnpm --filter @secondbrain/desktop build:app` hızlı bir debug `.app` üretir.

## Sürüm yayınlama

`desktop-v<sürüm>` biçiminde bir etiket push edilir (ör. `git tag desktop-v0.1.0 && git push origin desktop-v0.1.0`). `.github/workflows/desktop-release.yml` şunları üretip GitHub'da normal bir sürüme (ön sürüm değil; güncelleyici `releases/latest`'i okur) ekler:
- macOS: universal `.dmg` (Apple Silicon ve Intel)
- Windows: `.exe` (NSIS kurulum)
- Linux: AppImage

Sürüm numarası etiketten alınır.

## Beta kullanıcıları için kurulum

Adım adım kurulum ve imzasız uygulama uyarılarının geçilmesi: [docs/BETA-INSTALL.md](../../docs/BETA-INSTALL.md).

## Deep link (T4.1.5)

`routune://<yol>?<sorgu>` bağlantıları uygulamayı açar (açıksa öne getirir) ve ilgili ekrana gider. Örneğin `routune://verify-email?token=…` doğrulama ekranını açar. Bağlantılar sadece uygulama içinde gezinmeyi tetikler; başka şemalar ve `..` yol parçaları yok sayılır (`src/lib/desktop-links`). macOS şemayı uygulama paketinden kaydeder; Windows ve Linux açılışta kaydeder. İkinci bir pencere açılmaz (single-instance).

## Otomatik güncelleme (T4.1.4)

- Uygulama her açılışta `https://github.com/omerdn/secondbrainv4/releases/latest/download/latest.json` adresine bakar. Daha yeni bir sürüm varsa indirir, kurar ve yeniden başlar; bu sırada "Routune güncelleniyor" ekranı görünür. Kontrol ya da indirme başarısız olursa mevcut sürüm çalışmaya devam eder.
- Güncellemeler Tauri'nin kendi anahtarıyla imzalanır; açık anahtar `tauri.conf.json` içinde (`plugins.updater.pubkey`). İmzası tutmayan paket kurulmaz. Bu imza Apple ya da Windows sertifikası değildir; ilk kurulumdaki işletim sistemi uyarısı yine çıkar.
- Özel anahtar ve parolası GitHub secrets'ta durur: `TAURI_SIGNING_PRIVATE_KEY` ve `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Yedekleri sahibin Mac'inde, `~/.tauri/routune-updater.key` ve `~/.tauri/routune-updater.password` dosyalarında. **Anahtar kaybolursa** mevcut kurulumlar yeni sürümleri kabul etmez; kullanıcıların yeni sürümü bir kez elle kurması gerekir.
- `releases/latest` GitHub'daki en yeni **ön sürüm olmayan** release'i gösterir. Bu yüzden masaüstü release'leri normal sürüm olarak, mobil release'ler ise ön sürüm olarak yayınlanır.
