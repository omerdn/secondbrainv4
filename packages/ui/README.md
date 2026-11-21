# @secondbrain/ui

Tasarım sistemi (ADR-0017): token'lar, temalar ve (T0.4.3'ten itibaren) React Native bileşenleri.

- `src/tokens.ts`: renk paleti, anlamsal renkler (açık/koyu), tipografi, boşluk, köşe yarıçapı.
- `src/theme.ts`: tema → CSS değişkenleri (`--color-*`, `R G B` kanalları). Takım rengi `--color-team` ve okunabilir metin rengi `--color-team-foreground` olarak verilir.
- `src/tailwind-preset.ts`: NativeWind/Tailwind preset'i. Renk sınıfları değişkenlere bağlıdır (`bg-background`, `text-foreground`, `bg-team`…), bu yüzden tema değişince sınıflar değişmez; `dark:` varyantı gerekmez.

Uygulamada tema, `apps/client` içindeki `ThemeProvider` ile NativeWind'in `vars()` fonksiyonu üzerinden uygulanır.
