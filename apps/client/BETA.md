# Routune mobil beta: paketler ve kurulum (T4.2.5, ADR-0023)

Mağaza yayını yok; paketler GitHub Releases'tan dağıtılır. Yeni bir sürüm `mobile-v<sürüm>` etiketiyle yayınlanır:

```bash
git tag mobile-v0.1.0 && git push origin mobile-v0.1.0
```

`.github/workflows/mobile-release.yml` canlı API adresiyle şunları üretip bir **ön sürüme** (pre-release) ekler:

| Dosya                               | Ne için                                                                                     |
| ----------------------------------- | ------------------------------------------------------------------------------------------- |
| `routune-<sürüm>.apk`               | Android telefonlar. Kalıcı Routune anahtarıyla imzalı; yeni sürüm eskisinin üstüne kurulur. |
| `routune-<sürüm>-unsigned.ipa`      | iPhone (Sideloadly/AltStore ile).                                                           |
| `routune-<sürüm>-ios-simulator.zip` | Mac'te Xcode simülatörü.                                                                    |

Release'ler ön sürüm olarak yayınlanır, çünkü `releases/latest` her zaman en yeni masaüstü sürümünü göstermeli: masaüstü güncelleyicisi oradan okuyor (T4.1.4).

## Kurulum

Beta kullanıcılarına yönelik adım adım kurulum (Android, iPhone, masaüstü, web): [docs/BETA-INSTALL.md](../../docs/BETA-INSTALL.md).

- 22 Kasım 2026'da elle gönderilen ilk APK debug anahtarıyla imzalanmıştı. O sürümü kuranların, kalıcı anahtarla imzalanan ilk sürümden önce eskisini bir kez silmesi gerekir.
- **iOS simülatörü (Mac + Xcode):** zip'i açın ve şu komutla kurun:

  ```bash
  xcrun simctl install booted Routune.app
  ```

## Anahtarlar

- Android imza anahtarı GitHub secrets'ta durur: `ROUTUNE_KEYSTORE_BASE64`, `ROUTUNE_KEYSTORE_PASSWORD`, `ROUTUNE_KEY_ALIAS`, `ROUTUNE_KEY_PASSWORD`. Yedeği sahibin Mac'inde, `~/.routune-android/` klasöründe. **Anahtar kaybolursa** yeni APK eskisinin üstüne kurulamaz; herkesin eski uygulamayı silip yeniden kurması gerekir.
- İmzalama `plugins/with-release-signing.js` ile yapılır. `ROUTUNE_KEYSTORE_FILE` tanımlı değilse (yerel derlemeler) şablonun debug imzası kullanılır.
- Sürüm `APP_VERSION` (etiketten) ve derleme numarası `APP_BUILD_NUMBER` (CI çalışma numarası) ile `app.config.ts`'e girer. Android, yeni bir APK'yı eskinin üstüne ancak derleme numarası daha büyükse kurar.

## Uzaktan (OTA) güncellemeler (T4.2.6)

- Kurulu uygulamalar her açılışta `https://api.routune.dev/updates/manifest` adresine sorar (`expo-updates`). Yeni bir JS paketi varsa indirir ve **bir sonraki açılışta** uygular. Yani arayüz ve iş mantığı değişiklikleri yeni APK/IPA kurulmadan kullanıcıya ulaşır.
- `main`'e her merge'de deploy, o commit'in JS paketini iOS ve Android için dışa aktarıp sunucuya yükler (`.github/workflows/deploy.yml`, `server/crates/updates`).
- Güncellemeler **runtime sürümüne** bağlıdır. Bu sürüm native katmanın parmak izidir (`runtimeVersion.policy: fingerprint`). Native bir değişiklik (yeni native kütüphane, izin, ikon, Expo SDK) parmak izini değiştirir; eski uygulamalar o güncellemeyi hiç görmez. Bu durumda yeni bir `mobile-v*` sürümü (yeni APK/IPA) gerekir.
- Sunucu her runtime için son 3 güncellemeyi tutar. Güncelleme yoksa 204 döner ve uygulama elindekiyle devam eder.
- Kod imzalama kullanılmıyor; güvenliği HTTPS sağlıyor (ADR-0023).

## En düşük desteklenen sürüm (T4.2.7)

Native ya da protokol değişikliği eski kurulumları uyumsuz bırakırsa, sunucudaki `.env`'de ilgili `MIN_VERSION_DESKTOP` / `MIN_VERSION_IOS` / `MIN_VERSION_ANDROID` değeri yükseltilir (ör. `0.2.0`) ve sunucu yeniden başlatılır. Daha eski uygulamalar açılışta "Yeni sürüm gerekli" ekranında kalır; ekrandaki düğme `DOWNLOAD_URL_*` adresini açar. Kontrol başarısız olursa uygulama engellenmez.
