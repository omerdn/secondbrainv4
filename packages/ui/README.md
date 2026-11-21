# @secondbrain/ui

Tasarım sistemi (ADR-0017): token'lar, temalar ve React Native bileşenleri.

- `src/tokens.ts`: renk paleti, anlamsal renkler (açık/koyu), tipografi, boşluk, köşe yarıçapı.
- `src/theme.ts`: tema → CSS değişkenleri (`--color-*`, `R G B` kanalları). Takım rengi `--color-team` ve okunabilir metin rengi `--color-team-foreground` olarak verilir.
- `src/tailwind-preset.ts`: NativeWind/Tailwind preset'i. Renk sınıfları değişkenlere bağlıdır (`bg-background`, `text-foreground`, `bg-team`…), bu yüzden tema değişince sınıflar değişmez; `dark:` varyantı gerekmez.

Uygulamada tema, `apps/client` içindeki `ThemeProvider` ile NativeWind'in `vars()` fonksiyonu üzerinden uygulanır.

## Bileşenler (T0.4.3)
`Text`, `Button`, `Input`, `Card`, `Sheet` (telefonda alttan açılan sayfa, geniş ekranda diyalog), `Toast` (`ToastProvider` + `useToast`), `Avatar`, `TagChip` (takım etiketleri takım rengiyle dolu, kişisel etiketler çerçeveli; PRD E3), `EmptyState`.

- Yalnızca React Native bileşenleri; hover'a bağlı davranış yok (ADR-0012).
- Erişilebilirlik rolleri ve etiketleri her bileşende tanımlı. Metinler (ör. "Close") prop olarak verilir; çeviriyi çağıran taraf yapar (`apps/client/src/lib/i18n`, T0.4.7).
- Portallarda (web'de `Modal`) tema değişkenleri `ThemeScope` ile yeniden uygulanır.
- Geliştirme galerisi: istemcide `/dev/components` (yalnızca `__DEV__`).
- Testler: `jest-expo` + React Native Testing Library (`test/`).

Not (geliştirme): iOS'ta LogBox'un sarı uyarı şeridi ekranın altını kapatır ve sheet altbilgisi görünmüyormuş gibi olur. Uyarı NativeWind'in `ImageBackground` kullanımından gelir; şerit kapatılınca içerik görünür. Üretim build'inde bu şerit yoktur.
