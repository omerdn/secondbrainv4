# secondbrainv4

Görevleri, rutinleri, hedefleri, sayısal takipleri ve defterleri **tek bir merkezde, birbirine bağlı** yöneten; kişisel, arkadaşla ortak ve takım kullanımını tek bir workspace'te destekleyen bir "hayat işletim sistemi".

> Durum: **Planlama tamamlandı, geliştirme Faz 0'dan başlıyor.**

## Dokümanlar
| Doküman | İçerik |
|---|---|
| [docs/PRD.md](docs/PRD.md) | Ürün gereksinimleri ve numaralı kurallar |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Stack, repo yapısı, veri modeli, yetki modeli, hesap formülleri |
| [docs/DECISIONS.md](docs/DECISIONS.md) | Mimari karar kayıtları (ADR) |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Fazlar, epic'ler, görevler |
| [CLAUDE.md](CLAUDE.md) | Geliştirme iş akışı ve kurallar |

## Stack (özet)
- **Arayüz:** Expo (React Native) + react-native-web. iOS, Android ve web tek kod tabanından çıkar.
- **Masaüstü:** Tauri 2
- **Sunucu:** Rust (axum, sqlx), PostgreSQL
- **Gerçek zamanlı düzenleme:** Yjs + yrs

## Görev takibi
GitHub Project: https://github.com/users/omerdn/projects/4
