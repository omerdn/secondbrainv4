/**
 * Auth screen texts. Turkish only until the i18n setup (T0.4.7) moves them into catalogs; kept in
 * one place so that move is mechanical.
 */
export const t = {
  email: "E-posta",
  password: "Şifre",
  newPassword: "Yeni şifre",
  confirmPassword: "Şifre (tekrar)",
  displayName: "Adın",
  passwordHint: "En az 8 karakter.",
  login: {
    title: "Giriş yap",
    submit: "Giriş yap",
    forgot: "Şifremi unuttum",
    noAccount: "Hesabın yok mu?",
    toRegister: "Kayıt ol",
  },
  register: {
    title: "Hesap oluştur",
    submit: "Kayıt ol",
    haveAccount: "Zaten hesabın var mı?",
    toLogin: "Giriş yap",
  },
  forgot: {
    title: "Şifremi unuttum",
    intro: "Hesabının e-posta adresini yaz, şifre sıfırlama bağlantısı gönderelim.",
    submit: "Bağlantı gönder",
    sent: "Bu adrese kayıtlı bir hesap varsa sıfırlama bağlantısı gönderdik. Gelen kutunu kontrol et.",
    back: "Girişe dön",
  },
  reset: {
    title: "Yeni şifre belirle",
    submit: "Şifreyi değiştir",
    done: "Şifren değişti. Tüm cihazlarda oturum kapatıldı, yeni şifrenle giriş yap.",
    missingToken: "Bağlantı eksik veya bozuk. Yeni bir sıfırlama bağlantısı iste.",
    invalidToken: "Bağlantının süresi dolmuş veya daha önce kullanılmış. Yeni bir bağlantı iste.",
    requestNew: "Yeni bağlantı iste",
  },
  providers: {
    or: "veya",
    google: "Google ile devam et",
    apple: "Apple ile devam et",
    failed: {
      google: "Google ile giriş tamamlanamadı. Tekrar dene.",
      apple: "Apple ile giriş tamamlanamadı. Tekrar dene.",
    },
  },
  session: {
    unavailable: "Sunucuya ulaşılamadı.",
    retry: "Tekrar dene",
    signOut: "Çıkış yap",
  },
  validation: {
    required: "Bu alan zorunlu.",
    email: "Geçerli bir e-posta adresi yaz.",
    passwordShort: "Şifre en az 8 karakter olmalı.",
    passwordLong: "Şifre en fazla 128 karakter olabilir.",
    tooLong: "Çok uzun.",
    mismatch: "Şifreler aynı değil.",
    invalid: "Geçersiz değer.",
  },
  errors: {
    badCredentials: "E-posta veya şifre hatalı.",
    emailTaken: "Bu e-posta adresiyle zaten bir hesap var.",
    rateLimited: (seconds: number | null) =>
      seconds === null
        ? "Çok fazla deneme yaptın. Biraz sonra tekrar dene."
        : `Çok fazla deneme yaptın. ${seconds} saniye sonra tekrar dene.`,
    network: "Sunucuya ulaşılamadı. Bağlantını kontrol et.",
    generic: "Bir şeyler ters gitti. Tekrar dene.",
  },
} as const;
