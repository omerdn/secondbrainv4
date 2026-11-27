# Routune kapalı beta: kurulum rehberi (T4.4.1, ADR-0023, ADR-0024)

Bu doküman beta kullanıcıları içindir. Routune'u web'de, masaüstünde (macOS, Windows, Linux), Android'de ve iPhone'da nasıl kuracağınızı anlatır. Paketler mağazada değil, GitHub Releases'tadır: <https://github.com/omerdn/secondbrainv4/releases>.

> Masaüstü ve mobil paketler tüm geliştirme fazları bitince yayınlanır (ADR-0024). O zamana kadar Routune'u **web'de** kullanın.

Hangi platformda olursanız olun aynı hesapla giriş yaparsınız. İlk girişte (ve metinler güncellendiğinde) gizlilik politikası, KVKK aydınlatma metni ve kullanım koşullarını onaylamanız istenir.

## Web (önerilen)

1. Tarayıcıda <https://app.routune.dev> adresini açın.
2. Hesap oluşturun ya da Google/Apple ile giriş yapın.
3. Anlık bildirim için: **Bildirimler → Tercihler → "Bu tarayıcıda anlık bildirimler" → Aç** ve tarayıcının izin sorusuna "İzin ver" deyin. Chrome, Edge, Firefox ve Safari'de çalışır.
   - **iPhone/iPad:** Safari'de sayfayı açın, Paylaş → **Ana Ekrana Ekle** deyin ve Routune'u ana ekrandaki simgeden açın. iOS anlık bildirimi yalnız böyle eklenen web uygulamasında verir.

## Masaüstü

Release sayfasında `desktop-v…` sürümünü açın ve platformunuzun dosyasını indirin. Paketler imzasızdır (ücretli Apple/Windows sertifikası yok); ilk açılışta işletim sistemi uyarı verir, bu beklenen bir durumdur.

| Platform | Dosya | Kurulum |
|---|---|---|
| macOS (Apple Silicon ve Intel) | `.dmg` | `.dmg`'yi açın, Routune'u **Applications** klasörüne sürükleyin. İlk açılışta "geliştirici doğrulanamadı" uyarısı çıkarsa uygulamaya **sağ tıklayın → Aç → Aç**. "Aç" seçeneği yoksa **Sistem Ayarları → Gizlilik ve Güvenlik → Yine de Aç**. Bu yalnız bir kez gerekir. |
| Windows | `.exe` | Dosyayı çalıştırın. "Windows bilgisayarınızı korudu" (SmartScreen) uyarısında **Ek bilgi → Yine de çalıştır**. |
| Linux | `.AppImage` | `chmod +x Routune_*.AppImage` ile çalıştırma izni verin ve dosyayı açın. |

Masaüstü uygulaması yeni sürümleri kendisi bulur ve kurar; sonraki sürümler için bir şey yapmanız gerekmez.

## Android

1. Release sayfasında `mobile-v…` sürümünü açın ve `routune-<sürüm>.apk` dosyasını telefonunuza indirin.
2. Dosyayı açın. Telefon izin isterse indirdiğiniz uygulamaya (ör. Chrome, Dosyalar) **bilinmeyen kaynaklardan yükleme** iznini verin.
3. **Yükle** deyin.

Yeni sürümler eskisinin üstüne kurulur; verileriniz sunucuda olduğu için bir şey kaybolmaz.

## iPhone

Ücretli Apple Developer hesabı olmadığı için uygulama sizin Apple ID'nizle imzalanarak kurulur.

1. Bilgisayarınıza (Mac veya Windows) [Sideloadly](https://sideloadly.io) ya da [AltStore](https://altstore.io) kurun.
2. Release sayfasındaki `mobile-v…` sürümünden `routune-<sürüm>-unsigned.ipa` dosyasını indirin.
3. iPhone'u kabloyla bilgisayara bağlayın. Sideloadly/AltStore'da `.ipa` dosyasını seçin ve kendi Apple ID'nizle kurun.
4. iPhone'da **Ayarlar → Genel → VPN ve Aygıt Yönetimi** bölümünde kendi geliştirici profilinize **güvenin**.
5. iOS 16 ve sonrasında **Ayarlar → Gizlilik ve Güvenlik → Geliştirici Modu**'nu açmanız istenebilir; açın ve telefonu yeniden başlatın.
6. **7 günde bir yenileme:** ücretsiz Apple ID ile imza 7 gün geçerlidir. Süre dolunca uygulama açılmaz; Sideloadly'de aynı adımlarla yeniden kurun ya da AltStore'un otomatik yenilemesini açık tutun (bilgisayar ve telefon aynı Wi-Fi'da olmalı). Verileriniz sunucuda olduğu için yenilemede bir şey kaybolmaz.

## Güncellemeler

- **Web:** her zaman en güncel sürümdür.
- **Masaüstü:** kendini günceller.
- **Mobil:** küçük güncellemeler uygulama açılırken otomatik iner. Büyük güncellemelerde uygulama "yeni sürümü indir" ekranı gösterir; yukarıdaki adımlarla yeni paketi kurun.

## Sorun giderme

- **"Bağlantı yok" ekranı:** Routune yalnız çevrimiçi çalışır. İnternet bağlantınızı kontrol edip **Tekrar dene** deyin.
- **Giriş sonrası "Yasal metinler" ekranı:** metinler güncellendiğinde bir kez çıkar; okuyup onaylayın.
- **Tarayıcıda anlık bildirim gelmiyor:** tarayıcının site ayarlarında Routune için bildirim izni "İzin ver" olmalı; işletim sisteminin bildirim ayarlarında tarayıcıya izin verildiğinden emin olun.
- **iPhone'da uygulama açılmıyor:** 7 günlük imza süresi dolmuştur; yeniden kurun.

## Geliştiriciler için

Paketlerin üretimi ve anahtarlar: [apps/client/BETA.md](../apps/client/BETA.md) (mobil), [apps/desktop/README.md](../apps/desktop/README.md) (masaüstü).
