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

`desktop-v<sürüm>` biçiminde bir etiket push edilir (ör. `git tag desktop-v0.1.0 && git push origin desktop-v0.1.0`). `.github/workflows/desktop-release.yml` şunları üretip GitHub'da bir ön sürüme (pre-release) ekler:
- macOS: universal `.dmg` (Apple Silicon ve Intel)
- Windows: `.exe` (NSIS kurulum)
- Linux: AppImage

Sürüm numarası etiketten alınır.

## Beta kullanıcıları için kurulum

Paketler imzasızdır (ücretli Apple/Windows sertifikası yok). Bu yüzden ilk açılışta işletim sistemi uyarı verir:

- **macOS:** `.dmg`'yi açın ve Routune'u Applications klasörüne sürükleyin. İlk açılışta "geliştirici doğrulanamadı" uyarısı çıkarsa uygulamaya **sağ tıklayıp → Aç → Aç** deyin. Bu sadece bir kez gerekir. Uyarıda "Aç" seçeneği yoksa Sistem Ayarları → Gizlilik ve Güvenlik bölümündeki **Yine de Aç** düğmesini kullanın.
- **Windows:** `.exe`'yi çalıştırın. "Windows bilgisayarınızı korudu" (SmartScreen) uyarısında **Ek bilgi → Yine de çalıştır** deyin.
- **Linux:** AppImage dosyasına çalıştırma izni verin (`chmod +x Routune_*.AppImage`) ve çalıştırın.
