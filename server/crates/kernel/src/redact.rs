//! Best-effort removal of personal data from free-form text before it is logged or sent to
//! error tracking (ARCHITECTURE §15: "Kişisel veriler loglara yazılmaz").
//!
//! Structured request logs never contain personal data in the first place (see `app::http`).
//! This function is for text we do not control, mainly error messages from libraries, e.g.
//! Postgres: `Key (email)=(ali@example.com) already exists.`
//!
//! Replaced: e-mail addresses → `<email>`, IPv4 addresses → `<ip>`, Postgres detail values
//! `=(...)` → `=(***)`, the token after `Bearer` → `***`.
//! Known gaps: IPv6 addresses, names and free text are not detected. Never log request bodies,
//! headers or query strings, and do not rely on this function as the only safeguard.

/// Returns `input` with personal data patterns masked.
pub fn redact(input: &str) -> String {
    let without_pg_details = redact_pg_detail_values(input);
    redact_words(&without_pg_details)
}

/// `...=(value)...` → `...=(***)...` (Postgres constraint violation details).
fn redact_pg_detail_values(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("=(") {
        out.push_str(&rest[..start + 2]);
        let after = &rest[start + 2..];
        match after.find(')') {
            Some(end) => {
                out.push_str("***)");
                rest = &after[end + 1..];
            }
            None => {
                out.push_str("***");
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-' | '@')
}

/// Splits into runs of word / non-word characters and masks sensitive words.
fn redact_words(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut previous_word = "";
    let mut chars = input.char_indices().peekable();

    while let Some(&(start, c)) = chars.peek() {
        let word_run = is_word_char(c);
        let mut end = start;
        while let Some(&(i, c)) = chars.peek() {
            if is_word_char(c) != word_run {
                break;
            }
            end = i + c.len_utf8();
            chars.next();
        }
        let run = &input[start..end];

        if !word_run {
            out.push_str(run);
            continue;
        }

        // Sentence punctuation is not part of an address: "mail ali@x.com." → keep the final dot.
        let core = run.trim_end_matches('.');
        let trailing = &run[core.len()..];

        if previous_word.eq_ignore_ascii_case("bearer") {
            out.push_str("***");
        } else if is_email(core) {
            out.push_str("<email>");
        } else if is_ipv4(core) {
            out.push_str("<ip>");
        } else {
            out.push_str(core);
        }
        out.push_str(trailing);
        previous_word = core;
    }
    out
}

fn is_email(word: &str) -> bool {
    match word.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty()
                && !domain.contains('@')
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
        }
        None => false,
    }
}

fn is_ipv4(word: &str) -> bool {
    let parts: Vec<&str> = word.split('.').collect();
    parts.len() == 4
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.len() <= 3
                && p.bytes().all(|b| b.is_ascii_digit())
                && p.parse::<u16>().is_ok_and(|n| n <= 255)
        })
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn masks_postgres_unique_violation_details() {
        let msg = "duplicate key value violates unique constraint \"users_email_key\": \
                   Key (email)=(ali@example.com) already exists.";
        let out = redact(msg);
        assert!(!out.contains("ali@example.com"));
        assert!(out.contains("Key (email)=(***) already exists."));
        assert!(out.contains("users_email_key"));
    }

    #[test]
    fn masks_emails_and_keeps_sentence_punctuation() {
        assert_eq!(
            redact("notify ayse.yilmaz+test@mail.example.org."),
            "notify <email>."
        );
        assert_eq!(redact("user@localhost"), "user@localhost"); // no dot: not an address
    }

    #[test]
    fn masks_ipv4_but_not_versions_or_numbers() {
        assert_eq!(
            redact("connect to 10.0.0.5:5432 failed"),
            "connect to <ip>:5432 failed"
        );
        assert_eq!(redact("rustc 1.100.0"), "rustc 1.100.0");
        assert_eq!(redact("value 999.1.1.1"), "value 999.1.1.1");
        assert_eq!(redact("retry 3 of 5"), "retry 3 of 5");
    }

    #[test]
    fn masks_bearer_tokens() {
        assert_eq!(
            redact("header Authorization: Bearer abc.DEF-123 rejected"),
            "header Authorization: Bearer *** rejected"
        );
    }

    #[test]
    fn leaves_ordinary_text_and_unicode_alone() {
        let text = "Görev bulunamadı: rutin 'Gitar çalış' (id 01936d3c-7a8e-7cc1)";
        assert_eq!(redact(text), text);
    }

    #[test]
    fn handles_unterminated_detail() {
        assert_eq!(redact("Key (email)=(ali@x.co"), "Key (email)=(***");
    }

    proptest! {
        #[test]
        fn embedded_emails_never_survive(
            prefix in "[a-zA-Z ]{0,20}",
            local in "[a-z][a-z0-9._+-]{0,10}",
            domain in "[a-z]{1,10}\\.[a-z]{2,6}",
            suffix in "[a-zA-Z ,.]{0,20}",
        ) {
            let email = format!("{local}@{domain}");
            let out = redact(&format!("{prefix} {email} {suffix}"));
            prop_assert!(!out.contains(&email), "{}", out);
        }

        #[test]
        fn embedded_ipv4_never_survives(a in 0u8.., b in 0u8.., c in 0u8.., d in 0u8..) {
            let ip = format!("{a}.{b}.{c}.{d}");
            let out = redact(&format!("peer {ip} reset"));
            prop_assert_eq!(out, "peer <ip> reset");
        }

        #[test]
        fn is_idempotent(s in "\\PC{0,60}") {
            let once = redact(&s);
            prop_assert_eq!(redact(&once), once.clone());
        }

        #[test]
        fn text_without_at_digits_or_parens_is_unchanged(s in "[a-zA-Z çğışöüÇĞİŞÖÜ,;:!?'\"-]{0,60}") {
            prop_assume!(!s.to_ascii_lowercase().contains("bearer"));
            prop_assert_eq!(redact(&s), s);
        }
    }
}
