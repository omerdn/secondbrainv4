# Ürün Gereksinimleri Dokümanı (PRD)

> Proje: **secondbrainv4** (çalışma adı) · Sürüm: 1.0 · Tarih: 2026-11-21
> Bu doküman ürünün **ne** yapacağını tanımlar. **Nasıl** yapılacağı [ARCHITECTURE.md](ARCHITECTURE.md)'de, alınan kararların gerekçeleri [DECISIONS.md](DECISIONS.md)'de, sıralama [ROADMAP.md](ROADMAP.md)'de.
> Kurallar numaralıdır (ör. `G3`, `R5`). Issue'lar ve PR'lar bu numaralara atıf yapmalıdır.

---

## 1. Vizyon ve problem

İnsanlar hayatlarında aynı anda pek çok süreci yürütür: bir enstrüman öğrenmek, bir kursu takip etmek, ev işleri, iş görevleri, arkadaşla izlenen bir dizi. Ara verildiğinde **"nerede kalmıştım, kimle, hangi hedef için?"** sorusunun cevabı kaybolur.

- Rutin uygulamaları sadece rutinleri,
- Görev uygulamaları sadece görevleri,
- Not uygulamaları sadece notları takip eder.

Hiçbiri birbirine bağlı değildir ve bu yüzden bağlam kopar.

**Ürünün amacı:** Görevleri, rutinleri, hedefleri, sayısal takipleri ve notları **tek bir merkezde, birbirine bağlı** olarak yöneten; kişisel, arkadaşla ortak ve takım kullanımını **tek bir workspace** içinde destekleyen bir "hayat işletim sistemi" (Life OS).

## 2. Ürün ilkeleri

1. **Tek workspace.** Kişisel, ortak ve takım alanları ayrı workspace'ler değildir. Ayrım etiketler ve filtrelerle yapılır.
2. **Bağlam sürekliliği.** Her nesnenin son durumu ve süreç geçmişi tek bakışta görülebilir.
3. **Mahremiyet önce gelir.** Kullanıcı, bir eylemin görünürlüğü genişleteceğini o eylemi **yapmadan önce** bilir.
4. **Sayılar önemlidir.** İlerleme hesapları doğru, açıklanabilir ve tutarlıdır.
5. **Modülerlik.** Yeni takip türleri (kalori, dizi, bütçe…) çekirdeği değiştirmeden eklenir.
6. **Sadece çevrimiçi.** İnternet yoksa uygulama kullanılmaz (bkz. §14). Bu yüzden veri kaybı ve senkronizasyon çatışması yaşanmaz.

## 3. Hedef kitle ve sıralama

1. **Bireysel kullanıcılar ve arkadaşlar** (ilk hedef): hobiler, öğrenme, alışkanlıklar, ortak izlenen diziler.
2. **Aile ve ev** (doğal genişleme): ev işi rotasyonu, ortak rutinler, ortak harcama.
3. **Takımlar** (sonraki aşama): iş görevleri, periyodik iş rutinleri, efor takibi.

## 4. Sözlük

| Terim | Anlamı |
|---|---|
| **Nesne** | Uygulamadaki her takip edilebilir şeyin ortak tabanı: görev, rutin, hedef, takip, defter, sayfa ve modül nesneleri. |
| **Görev** | Başlangıç/bitiş tarihi olabilen, en az bir alt kırılımı olan iş. |
| **Alt kırılım** | Görevin to-do maddesi. Görevin ilerlemesi alt kırılımlardan hesaplanır. |
| **Rutin** | Tekrar kuralına göre tekrarlanan ve her tekrarı işaretlenen iş. |
| **Tekrar** | Bir rutinin belirli bir güne düşen örneği (ör. "21 Kasım'daki çöp atma"). |
| **Hedef** | Başlangıç ve bitiş tarihi zorunlu olan, bağlı nesnelerin ilerlemesinden oluşan amaç. |
| **Takip** | Sayısal ilerleme tanımı: birim, hedef değer, dönem, toplama yöntemi, yön. |
| **Kayıt** | Bir takibe girilen tek veri noktası (+1 bardak, +25 dk, −450 TL). |
| **Defter** | Sayfaları gruplayan not defteri. Sahibi bir kişi veya bir takımdır. |
| **Sayfa** | Defterdeki tek bir not. Birden fazla kişi gerçek zamanlı düzenleyebilir. |
| **Süreç sayfası** | Bir nesnenin varsayılan sayfası. Kayıt notları buraya tarihli satır olarak eklenir. |
| **Bağlantı** | Bir sayfanın veya nesnenin başka bir nesneye bağlanması (link). Ek bir işlevi yoktur, ancak görünürlüğü etkiler (bkz. §7). |
| **Work Session** | Pomodoro ve kronometre dilimlerinden oluşan çalışma oturumu. |
| **Etiket** | Filtreleme için nesnelere takılan işaret. Sahibi bir kişi veya bir takımdır. |
| **Kayıtlı görünüm** | Kaydedilmiş filtre. Workspace'lerin yerini tutar. |

## 5. Çekirdek kavramlar

### 5.1 Nesne, sahiplik ve üyelik
- `N1` Her nesnenin tek bir **sahibi** vardır: bir kişi ya da bir takım.
- `N2` Bir nesneye kişiler şu rollerle bağlanabilir:
  - **sahip / yönetici:** nesneyi paylaşabilir, silebilir, üye ekleyip çıkarabilir.
  - **sorumlu (atanan):** işi yapması beklenen kişi.
  - **izleyici:** görür ama işin sorumlusu değildir.
- `N3` Bir nesneyi oluşturan kişi, otomatik olarak onun sorumlusu olmak zorunda değildir. Bu özellikle takımlarda önemli: yönetici bir rutin oluşturup başkalarını sorumlu yapabilir.
- `N4` Silme işlemi önce çöp kutusuna gider (30 gün). Arşivleme ayrı bir durumdur.

### 5.2 Etiketler
- `E1` Etiketin sahibi bir **kişi** (kişisel etiket) ya da bir **takım** (takım etiketi) olabilir.
- `E2` Kişisel etiketleri sadece sahibi görür. Ortak bir nesneye kişisel etiket takılsa bile diğer üyeler o etiketi görmez.
- `E3` Takım etiketlerini takım üyeleri görür. Takım etiketleri, takımın rengi ve ayırt edici bir stille gösterilir. Ayrım ayrı listelerle değil, UI ile yapılır.
- `E4` Etiket ekranında kullanıcının görebildiği tüm etiketler listelenir. Bir etiket seçilince o etikete sahip nesneler gösterilir.

### 5.3 Filtreler ve kayıtlı görünümler
- `F1` Uygulama genelinde filtreleme yapılabilir: etiket, nesne türü, kişi ("seçtiğim arkadaşlarla ortak olanlar"), takım, tarih, durum.
- `F2` Filtreler **kayıtlı görünüm** olarak saklanabilir ve kenar çubuğuna sabitlenebilir.
- `F3` Arkadaş filtresinde sadece arkadaş olunan kişiler seçilebilir.

### 5.4 Bağlantılar
- `B1` Sayfalar görevlere, rutinlere, hedeflere ve takiplere bağlanabilir. Görevler, rutinler ve takipler hedeflere bağlanabilir.
- `B2` Bağlantının kendisi bir işlev taşımaz. Görünürlüğe etkisi §7'de tanımlıdır.
- `B3` Bir sayfanın metni içinde bir nesneye atıf yapmak (`@görev`) bağlantı **değildir** ve kimseye erişim vermez.

### 5.5 Takip ve kayıt (çekirdek motor)
- `T1` **Takip**, şu alanlardan oluşan bir tanımdır:
  - **birim:** adet, dakika, sayfa, bölüm, kalori, TL…
  - **hedef değer**
  - **dönem:** günlük, haftalık, aylık, toplam veya hedef aralığı
  - **toplama yöntemi:** toplam, sayım, son değer, en yüksek değer
  - **yön:** artmalı (kitap sayfası) ya da sınırın altında kalmalı (bütçe, kalori)
- `T2` **Kayıt**, bir takibe girilen değerdir. İsteğe bağlı olarak kısa bir not taşıyabilir.
- `T3` Kayıt notu, nesnenin **süreç sayfasına** tarihli bir satır olarak eklenir. Notun tek kaynağı o satırdır: sayfada düzeltilen not, kayıtta da düzelmiş görünür.
- `T4` Rutinler, görevler ve hedefler ilerleme hesabını bu motor üzerinden yapar. Ek modüller (kalori, dizi, bütçe…) da bu motorun özelleşmiş halleridir.
- `T5` Kullanıcı genel amaçlı bir "Takip" nesnesi oluşturabilir. Ürün pazarlamada ise "kitap takibi", "kurs takibi", "kalori takibi" gibi modüller olarak sunulur.

### 5.6 Süreç sayfası
- `S1` Her nesnenin isteğe bağlı bir süreç sayfası olabilir. İlk not yazıldığında veya ilk kayıt notu girildiğinde otomatik oluşur.
- `S2` Süreç sayfası bir defterin içinde durur:
  - kişisel nesnelerde kullanıcının varsayılan defterinde,
  - takım nesnelerinde takımın varsayılan defterinde.
- `S3` Nesne kartında ve detayında **son kayıt / son durum satırı** görünür ("Barre'da kaldım · 3 gün önce").

### 5.7 Aktivite geçmişi
- `A1` Nesneler üzerindeki her anlamlı değişiklik (oluşturma, tamamlama, atama, paylaşım, bağlama) aktivite olarak kaydedilir: kim, ne, ne zaman.
- `A2` Kullanıcı, sadece görebildiği nesnelerin aktivitelerini görür.

## 6. Modüller (çekirdek ürün)

### 6.1 Ana sayfa: "Bugün"
- `H1` Bugünün rutinleri; doğrudan buradan işaretlenebilir.
- `H2` Yaklaşan ve geciken görevler.
- `H3` Aktif hedefler: gerçekleşen ilerleme ve tempo.
- `H4` **Unutulanlar:** X gündür dokunulmayan aktif nesneler (X ayarlanabilir, varsayılan 7).
- `H5` Son durum satırlarıyla birlikte "Kaldığın yerden devam et" listesi.
- `H6` Açık bir Work Session varsa onun kısa görünümü.

### 6.2 Görevler
- `G1` Bir görevde şunlar bulunur: başlık, açıklama, başlangıç ve bitiş tarihi (opsiyonel), etiketler, bağlı sayfalar, üyeler.
- `G2` Her görevin **en az 1 alt kırılımı** vardır. Kullanıcı alt kırılım eklemeden bir görev oluşturursa, görevin başlığıyla otomatik bir alt kırılım oluşturulur.
- `G3` Görevin tamamlanma yüzdesi = tamamlanan alt kırılım / toplam alt kırılım.
- `G4` Alt kırılımlara ayrıca bir veya birden fazla kişi atanabilir. Atanan kişilerden **biri** işaretlediğinde alt kırılım tamamlanmış olur.
- `G5` Göreve ortak eklendiğinde ortağa bir davet bildirimi gider. Ortak kabul ederse eklenir, reddederse hiçbir şey olmaz.
- `G6` **Kanban kişiseldir.**
  - Her kullanıcının kendi sütunları vardır ve özelleştirilebilir.
  - Bir görevin kullanıcının hangi sütununda durduğu sadece o kullanıcıyı ilgilendirir. Görevin gerçek ilerlemesini etkilemez (`G3`).
  - Kullanıcıya yeni bir ortak görev geldiğinde, görev o kullanıcının ilk sütununa düşer.
- `G7` Görünümler: kanban, liste, takvim.

### 6.3 Rutinler
- `R1` Tekrar türleri:
  - günlük,
  - haftalık (seçili günler),
  - aylık (seçili günler, ör. 15 ve 20; "ayın son günü" de seçilebilir),
  - yıllık (seçili tarihler),
  - "her N günde/haftada bir".
- `R2` Başlangıç ve bitiş tarihi opsiyoneldir. Rutinin bir **saat dilimi** vardır; günün ne zaman bittiği bu saat dilimine göre belirlenir.
- `R3` **Günlük hedef sayısı:** bir rutin günde birden fazla kez yapılabilir (ör. su iç 8/gün). Varsayılan 1.
- `R4` **Kısmi tamamlama sayılır:** 8 hedefli bir günde 5 yapıldıysa, o gün 5 olarak sayılır (haftada 13/25 gibi).
- `R5` Ortak rutinlerde tamamlanma modu seçilir:
  - **Ortak havuz:** Kim yaparsa yapsın, ortak sayaca eklenir. Hedef 1 ise "tek kişi yeterli" anlamına gelir.
  - **Herkes ayrı:** Her sorumlu kendi hedefini tamamlamak zorundadır.
- `R6` **Nöbet/rotasyon modu:** Her tekrarın sorumlusu, sıralı üye listesinden döner şekilde belirlenir (her tekrarda veya her hafta). Kişiler aralarında takas yapabilir.
- `R7` **Geçmişe dönük işaretleme** en fazla **3 gün** geriye yapılabilir. Bu kural sunucu tarafında, rutinin saat diliminde uygulanır.
- `R8` **Isı haritası:** GitHub katkı karelerine benzer bir görünüm. Kare rengi o günün tamamlanma oranına göre koyulaşır.
- `R9` **Streak:** Ardışık **planlanmış** tekrarların **tam** tamamlanmasıdır. Planlanmamış günler seriyi bozmaz, kısmi günler ise seriyi devam ettirmez. Bugünün tekrarı, gün bitene kadar seriyi bozmaz.
- `R10` Ortak rutinlerin streak'i ortaktır. Bunun için ayrı bir "ortak streak" özelliği yoktur.
- `R11` Rutinin planı değiştirildiğinde geçmiş tekrarlar ve geçmiş ısı haritası korunur.
- `R12` Bir rutine sayfa bağlanabilir ve kişiler (sorumlu, izleyici) eklenebilir. Rutinlerin alt kırılımı yoktur.
- `R13` Rutinler için hatırlatma saatleri tanımlanabilir (bildirim gönderilir).

### 6.4 Hedefler
- `D1` Başlangıç ve bitiş tarihi **zorunludur**.
- `D2` Hedefe görevler, rutinler, takipler ve (modüller aracılığıyla) modül nesneleri bağlanabilir. Sayfa da bağlanabilir, ancak sayfalar ilerlemeye etki etmez.
- `D3` İlerleme, bağlı nesnelerin **ağırlıklı ortalamasıdır**. Varsayılan olarak tüm ağırlıklar eşittir, kullanıcı değiştirebilir. Formüller [ARCHITECTURE.md §7](ARCHITECTURE.md)'de.
- `D4` İki gösterge birlikte gösterilir:
  - **Gerçekleşen:** tüm dönem üzerinden, ör. "%30".
  - **Tempo:** bugüne kadar beklenen ilerlemeye göre, ör. "Planın %95'indesin".
- `D5` Rutinler hedefe, sadece **hedef ve rutin tarih aralıklarının kesişimi** içindeki tekrarlarla katkı verir.
- `D6` Hedef ekranında her bağlı nesnenin katkısı ayrı ayrı görünür. Hesap açıklanabilir olmalıdır.

### 6.5 Takipler
- `K1` Kullanıcı genel bir takip oluşturabilir (`T1`). Kayıt girerken değeri ve isteğe bağlı bir notu yazar.
- `K2` "Sınırın altında kalmalı" yönündeki takiplerde (bütçe, kalori) dönem içi kullanım ve kalan miktar gösterilir.
- `K3` Takip geçmişi grafikle gösterilir.

### 6.6 Defterler ve sayfalar
- `Y1` Not modülünün yapısı **Defter → Sayfa** şeklindedir. Bir kullanıcının birden fazla defteri olabilir.
- `Y2` İki sahiplik türü vardır:
  - **Kişisel defter:** Sahibi kişidir. Varsayılan olarak sadece sahibi görür. Sahibi, arkadaşlarını deftere ekleyebilir; bu durumda defter UI'da "ortak" rozetiyle görünür.
  - **Takım defteri:** Sahibi takımdır. Görünürlük defter bazında seçilir: tüm takım veya seçili kişiler.
- `Y3` Sayfalar gerçek zamanlı olarak birden fazla kişi tarafından düzenlenebilir. **Hiçbir koşulda veri kaybı olmaz.** Canlı imleçler gösterilir ve sürüm geçmişinden eski bir sürüme dönülebilir.
- `Y4` Zengin metin desteklenir: başlıklar, listeler, yapılacaklar listesi, kod, alıntı, tablo. Dış kaynak embed'leri de desteklenir: YouTube, Google Drive, Spotify, SoundCloud ve link önizlemesi. **Dosya yükleme yoktur** (bkz. §17).
- `Y5` Sayfa sahipliği: Sayfanın sahibi, bağlı olduğu defterin sahibidir. **Düzenleme yetkisi sahiplik değildir** (bkz. §7).

### 6.7 Work Session
- `W1` Bir oturum, kullanıcı **"Oturumu kapat"** diyene kadar açık kalır. Oturum, ardışık **dilimlerden** oluşur.
- `W2` Dilim modları:
  - **pomodoro** (geri sayım; çalışma, kısa mola ve uzun mola süreleri ayarlanabilir),
  - **kronometre** (ileri sayım),
  - **mola**.
- `W3` Bir dilim oluşturulurken isteğe bağlı olarak bir nesne seçilir (görev, rutin, takip…). Duraklatıldığında veya durdurulduğunda bağlı nesne değiştirilebilir; bu değişiklik **yeni bir dilim** başlatır.
- `W4` Oturumun altında dilimlerin log'u görünür ("45 dk · Gitar kursu → 5 dk mola → 25 dk · Rapor").
- `W5` Work Session **hiçbir şeyi kendiliğinden tamamlamaz**. Süreler istatistik olarak kaydedilir: günlük toplam, nesne bazında toplam, geçmiş oturumlar.
- `W6` **Ortak oturum:**
  - Arkadaşlar veya aynı nesnenin üyeleri aynı timer'ı görür. Herkes duraklatabilir ve durdurabilir.
  - Bağlı nesneyi her katılımcı **kendisi** seçer, herkesin log'u kendine aittir.

### 6.8 Takvim
- `C1` Uygulamanın kendi takvim görünümü vardır: görevlerin başlangıç/bitiş tarihleri, rutin tekrarları ve hedef aralıkları (gün, hafta ve ay görünümleri).
- `C2` Dış takvim entegrasyonu ilerideki bir fazda gelecek (bkz. ROADMAP): iCal feed, ardından Google ve Apple Takvim.

### 6.9 Global arama
- `AR1` Tüm nesne başlıkları, açıklamalar, sayfa içerikleri ve etiketler aranabilir. Türkçe dil desteği vardır.
- `AR2` Sonuçlar **sadece kullanıcının görebildiği** nesnelerden gelir.

## 7. Paylaşım ve görünürlük kuralları

Bu bölüm ürünün en kritik bölümüdür. Her kural, sunucu tarafında otomatik testlerle güvence altına alınmalıdır.

- `V1` **Paylaşım ön koşulu:** Bir kişiyle nesne paylaşmak için o kişiyle **arkadaş** olmak veya **aynı takımda** olmak gerekir.
- `V2` **Sayfayı görebilenler** = defterin üyeleri ∪ sayfanın bağlı olduğu **her** nesneyi görebilenler.
- `V3` Bağlantı yoluyla (miras) gelen erişim **görüntüleme + düzenleme** yetkisidir. Paylaşma, bağlama, üye yönetimi ve silme yetkisi **vermez**.
- `V4` **Sayfayı bir nesneye bağlama** yetkisi sadece sayfanın sahibinde vardır. Takım sayfalarında bu yetki, yetkilendirilmiş rollere de verilebilir. Bağlayan kişinin hedef nesneyi de görebiliyor olması gerekir.
- `V5` **Ortak bir nesneye üye ekleme** yetkisi sadece nesnenin sahibinde vardır. Takımlarda bu yetki rol matrisinden ayarlanır.
- `V6` Bir sayfayı veya nesneyi **silme** yetkisi sadece sahipte vardır.
- `V7` **Önceden uyarı:**
  - Görünürlüğü genişletecek her eylemden önce, kimlerin yeni erişim kazanacağı açıkça gösterilir ve onay istenir. Örnek: *"Bu sayfayı bağlarsan Ayşe ve Mehmet de görebilecek."*
  - Bu durum, bağlı sayfaları olan bir nesneye üye eklerken de geçerlidir. Örnek: *"Bu göreve bağlı 2 sayfa da bu kişiyle paylaşılacak."*
- `V8` **Otomatik yeniden hesaplama:** Şu durumlardan herhangi birinde erişim yeniden hesaplanır: bağlama ya da bağlantı kaldırma, üye ekleme ya da çıkarma, defter üyeliği değişikliği, takım rolü değişikliği, takım nesnesinin görünürlük modunun değişmesi. Kaldırılan bir bağlantıyla gelen erişim **otomatik olarak düşer**.
- `V9` **Gizli bağlantılar görünmez:** Bir sayfayı görebilen ama bağlı olduğu nesneyi göremeyen kişi, o bağlantının **varlığından bile haberdar olmaz**. Başlık, sayı veya "erişiminiz olmayan bağlantı" gibi bir ibare gösterilmez.
- `V10` **Erişim kaybı:** Erişimi kalkan bir kullanıcının açık oturumları anında kesilir. Örneğin açık bir sayfa düzenleme bağlantısı varsa sonlandırılır.
- `V11` **"Bunu neden görüyorum?"** Kullanıcı bir nesneye hangi yoldan eriştiğini görebilir: sahip, üye, takım rolü veya bağlantı.
- `V12` **Takım nesnelerinin görünürlük modu** oluşturulurken seçilir:
  - **Tüm takım**,
  - **Sadece atananlar.** Bu modda oluşturan kişi, atananlar ve yönetici ile üstündeki roller görür.

## 8. Arkadaşlık ve sosyal özellikler

- `AK1` Arkadaşlık isteği gönderilebilir, kabul veya reddedilebilir. Arkadaşlıktan çıkarma ve **engelleme** yapılabilir.
- `AK2` İstek spamına karşı hız sınırı uygulanır. Reddedilen bir isteğin bekleme süresi dolmadan aynı kişiye tekrar istek gönderilemez. Engellenen kişi istek gönderemez, dürtemez ve paylaşım yapamaz.
- `AK3` **Dürtme:** Bir kişi aynı kişiyi günde en fazla **1 kez** dürtebilir.
- `AK4` **Emoji tepkisi:** Arkadaşların tamamladığı rutinlere, görevlere ve kayıtlara emoji ile tepki verilebilir.
- `AK5` Arkadaşla ortak takip edilen nesnelerde (ör. dizi) iki tarafın ilerlemesi yan yana görünür.

## 9. Takımlar

- `TK1` Takım oluşturulabilir. Takımın bir adı, rengi ve isteğe bağlı bir logosu olur.
- `TK2` **Katılım:** Takıma eklenen kişiye bildirim gider. Kişi kabul ederse takıma katılır, reddederse katılmaz. Takım içi atamalar ise onaysız, doğrudan yapılır.
- `TK3` Roller: **admin, yönetici, üye, misafir.** Admin, **rol × yetki matrisini** düzenler. Matristeki yetkilere örnekler: nesne oluşturma, atama yapma, üye davet etme, takım etiketi yönetme, takım defteri oluşturma, aktivite log'unu görme.
- `TK4` Yöneticinin oluşturduğu nesnelerde sorumlular ayrıca seçilir. Oluşturan kişi sorumlu olmak zorunda değildir (`N3`). Yönetici ve üstündeki roller, tamamlanma durumunu her zaman görebilir.
- `TK5` **Takım ayarları sayfası:** Admin ve yöneticiler üyeleri, rolleri, yetki matrisini, etiketleri ve defterleri buradan yönetir. Bu bir workspace değil, bir ayarlar ekranıdır.
- `TK6` **Takım panosu:** Takım filtresiyle açılan bir görünüm. Takımın aktif hedefleri, rutin uyumu, geciken görevler ve aktivite log'u burada görünür.
- `TK7` Bir kişi takımdan ayrıldığında takıma ait nesneler takımda kalır ve kişinin bu nesnelere erişimi düşer.

## 10. Bildirimler

- `BL1` Kanallar:
  - uygulama içi bildirim merkezi,
  - push (masaüstü ve mobil),
  - e-posta (kritik bildirimler ve özetler için).
- `BL2` Bildirim türleri: davetler (görev ve rutin ortaklığı, takım katılımı), atamalar, rutin hatırlatmaları, dürtme, emoji tepkisi, paylaşım değişiklikleri, "unutulanlar" hatırlatması, haftalık özet.
- `BL3` Kullanıcı her bildirim türü için kanal tercihini ayarlayabilir.

## 11. Özetler

- `OZ1` **Haftalık özet:** Tamamlanan rutinler, streak'ler, hedeflerdeki ilerleme, nesne bazında Work Session süreleri ve unutulanlar.
- `OZ2` **Yıl sonu Wrapped:** Paylaşılabilir görsel bir yıllık özet.

## 12. Ek modüller

- `M1` Ek modüller çekirdek motorun üzerine kurulur. Kullanıcı veya takım bazında açılıp kapatılabilir. İleride ücretli paketlere dahil edilebilir.
- `M2` İlk modül paketi:
  - **Abonelik takibi:** Yenileme tarihleri, aylık veya yıllık tekrar ve hatırlatma. Abonelik ücreti bütçeye otomatik kayıt olarak düşer.
  - **Aylık bütçe takibi:** "Sınırın altında kalmalı" yönünde bir takip. Kategori bazlı olabilir.
- `M3` Sonraki modüller: kalori takibi, dizi/film takibi (arkadaşla ortak ilerleme), kitap ve kurs takibi, eşya takibi ("evde neyi nereye koydum").

## 13. Hesap, mahremiyet ve yasal

- `HS1` Kayıt yöntemleri: e-posta + şifre, Google ile giriş, Apple ile giriş.
- `HS2` **Hesap silme akışı:**
  1. Kullanıcıya, sahibi olduğu ortak nesnelerin sahipliğini devretmesi veya ortaklıklardan çıkması için rehberlik edilir.
  2. Kullanıcı bunları yapmazsa, **30 gün** sonra sahiplik en eski ortağa otomatik devredilir ve hesap silinir (KVKK/GDPR silme hakkı).
- `HS3` Kullanıcı verilerini dışa aktarabilir (JSON ve Markdown).
- `HS4` Ürün tıbbi kayıt sistemi değildir ve o şekilde konumlandırılmaz.
- `HS5` Gizlilik politikası ve KVKK aydınlatma metni, kapalı betadan önce hazır olmalıdır.

## 14. Bağlantı politikası (sadece çevrimiçi)

- `BP1` Uygulama açılırken bağlantı yoksa içeriğe girilmez. "Bağlantı yok" ekranı ve "Tekrar dene" butonu gösterilir. Önbellekten de bir şey gösterilmez.
- `BP2` Uygulama açıkken bağlantı koparsa, ekranı kapatan bir katman çıkar ("Bağlantı koptu, yeniden bağlanılıyor…") ve hiçbir işlem yapılamaz. Bağlantı geldiğinde katman kalkar ve veriler tazelenir.
- `BP3` Kopma anında gönderilmekte olan bir işlem sessizce kaybolmaz: ya onaylanır ya da "kaydedilemedi" hatası gösterilir. Formda yazılanlar korunur.
- `BP4` Bir sayfada kopmadan hemen önce yazılan değişiklikler, bağlantı gelince otomatik olarak gönderilir.
- `BP5` Bağlantı kontrolü saniye başı yapılmaz. Kontrol, işletim sistemi olayları, WebSocket canlılık sinyali ve başarısız istekler üzerinden yapılır.

## 15. Platformlar ve dağıtım

- `PL1` Platformlar: web, macOS, Windows, Linux, iOS, Android. Tek bir arayüz kod tabanından çıkar.
- `PL2` Geliştirme sırası **önce web**, ancak arayüz baştan mobile hazır yazılır. Sonra masaüstü, sonra mobil.
- `PL3` **Kapalı beta** eş-dost çevresiyle yapılır. Dağıtım için `.dmg`, `.exe`, `.apk` ve `.ipa` dosyaları üretilir.
  - iOS betası, Apple Developer hesabı olan kişiler veya TestFlight üzerinden dağıtılır.
  - İmzasız masaüstü build'lerinde işletim sistemi uyarı verir. Beta için bu kabul edilmiştir.
- `PL4` Mağaza yayını betadan sonra yapılır.

## 16. Gelir modeli (taslak)

- `GM1` Freemium + abonelik. Ücretli paket adayları: ek modül paketleri, takım özellikleri, gelişmiş özetler ve istatistikler, ileride dosya yükleme kotası.
- `GM2` **Kısıt:** Apple ve Google, uygulama içinde satılan dijital aboneliklerde kendi ödeme sistemlerinin kullanılmasını şart koşar. Bu yüzden mobilde mağaza içi satın alma (IAP) gerekir; web ve masaüstünde ödeme sağlayıcı (Stripe vb.) kullanılabilir. Detaylar monetizasyon fazında netleşecek.
- `GM3` Paket içerikleri ve fiyatlar henüz belirlenmedi.

## 17. Kapsam dışı ve backlog

**Kapsam dışı (kararlaştırıldı):**
- Çevrimdışı kullanım (`BP1`–`BP5`).
- Yapay zekâ özellikleri: maliyet nedeniyle şimdilik yok.
- Dosya, ses ve fotoğraf yükleme: embed ile çözülür.
- Tıbbi kayıt kullanımı.

**Backlog (sonraki fazlar):**
- Notion'dan tek yönlü içe aktarma (Notion → uygulama).
- Google ve Apple Takvim ile iki yönlü entegrasyon.
- Şablon galerisi.
- Ana ekran widget'ları ve macOS menü çubuğu.
- Doğal dille nesne oluşturma.
- Kotalı dosya yükleme (ücretli).
- Zanzibar tabanlı yetki servisine geçiş (sadece ölçek gerektirirse).
- Todoist, Habitica ve benzeri uygulamalardan içe aktarma.
