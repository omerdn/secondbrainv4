# @secondbrain/api-client

API'ye tipli HTTP istemcisi (T0.4.4). Tipler `@secondbrain/api-types`'tan gelir (Rust DTO'larından üretilir), sunucu sözleşmesi bozulursa istemci typecheck'i kırılır.

- **`ApiClient`**: JSON istek/yanıt. Her hata `ApiError`'dur (`code`: sunucu kodları + `network` + `unknown`, `status`, alan hataları `fields`, `retryAfter`, `retryable`).
- **Kimlik**: web'de `HttpOnly` cookie (`credentials: "include"`) ve mutasyonlarda `X-CSRF-Token`; native'de `Authorization: Bearer` (ARCHITECTURE §15).
- **Idempotency** (ADR-0003, ARCHITECTURE §10.2): her mutasyon `Idempotency-Key` taşır. `useIdempotentMutation`, başarısız bir gönderim aynı girdiyle tekrarlanınca aynı anahtarı kullanır (sunucu kaydedilmiş sonucu döner, işlem iki kez uygulanmaz). Girdi değişince veya başarıdan sonra yeni anahtar üretilir.
- **TanStack Query**: `createQueryClient()`: çevrimdışı kuyruk yok (`networkMode: "always"`); sorgular yalnızca yeniden denenebilir hatalarda en fazla 2 kez tekrar edilir; mutasyonlar otomatik tekrar edilmez.
- **React**: `ApiProvider`, `useApi()`, `useIdempotentMutation()` (`@secondbrain/api-client/react`).

React Native (Hermes) ortamında Web Crypto yoktur: uygulama, `ApiClient` ve `ApiProvider`'a `expo-crypto` tabanlı bir anahtar üretici verir (`apps/client/src/lib/api.ts`).
