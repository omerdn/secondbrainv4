//! Fractional indexing for board order (PRD G6, T2.1.3): moving a card rewrites one key instead of
//! renumbering its column. Keys are base-62 strings compared byte by byte and never end in `0`,
//! so there is always room below any key. Text keys do not run out of precision as floats do.

const DIGITS: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const BASE: usize = DIGITS.len();

/// Keys longer than this make the service renumber the column with [`even_keys`].
pub const MAX_KEY_LEN: usize = 64;

fn digit(byte: u8) -> usize {
    DIGITS.iter().position(|d| *d == byte).unwrap_or(0)
}

fn midpoint(a: &[u8], b: Option<&[u8]>) -> Vec<u8> {
    if let Some(b) = b {
        let shared = b
            .iter()
            .enumerate()
            .take_while(|(i, byte)| a.get(*i).copied().unwrap_or(b'0') == **byte)
            .count();
        if shared > 0 {
            let mut key = b[..shared].to_vec();
            key.extend(midpoint(a.get(shared..).unwrap_or(&[]), Some(&b[shared..])));
            return key;
        }
    }
    let low = a.first().map_or(0, |byte| digit(*byte));
    let high = b.and_then(<[u8]>::first).map_or(BASE, |byte| digit(*byte));
    if high - low > 1 {
        return vec![DIGITS[(low + high).div_ceil(2)]];
    }
    if let Some(b) = b
        && b.len() > 1
    {
        return vec![b[0]];
    }
    let mut key = vec![DIGITS[low]];
    key.extend(midpoint(a.get(1..).unwrap_or(&[]), None));
    key
}

/// A key strictly between `before` and `after`; an absent side is open. Both must be valid keys
/// with `before < after`.
#[must_use]
pub fn key_between(before: Option<&str>, after: Option<&str>) -> String {
    let key = midpoint(
        before.map_or(&[][..], str::as_bytes),
        after.map(str::as_bytes),
    );
    String::from_utf8_lossy(&key).into_owned()
}

/// `count` short, evenly formed keys in ascending order.
#[must_use]
pub fn even_keys(count: usize) -> Vec<String> {
    let mut width = 1;
    let mut capacity = BASE - 1;
    while capacity < count {
        width += 1;
        capacity = capacity.saturating_mul(BASE);
    }
    (1..=count)
        .map(|index| {
            let mut rest = index;
            let mut key = vec![b'0'; width + 1];
            key[width] = b'V';
            for slot in key[..width].iter_mut().rev() {
                *slot = DIGITS[rest % BASE];
                rest /= BASE;
            }
            String::from_utf8_lossy(&key).into_owned()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn valid(key: &str) -> bool {
        !key.is_empty() && !key.ends_with('0') && key.bytes().all(|b| DIGITS.contains(&b))
    }

    #[test]
    fn first_key_and_open_sides() {
        let first = key_between(None, None);
        assert!(valid(&first));
        assert!(key_between(None, Some(&first)) < first);
        assert!(key_between(Some(&first), None) > first);
    }

    #[test]
    fn adjacent_keys_still_have_room() {
        let key = key_between(Some("a"), Some("b"));
        assert!("a" < key.as_str() && key.as_str() < "b");
        let key = key_between(None, Some("1"));
        assert!(key.as_str() < "1" && valid(&key));
        let key = key_between(Some("z"), None);
        assert!(key.as_str() > "z" && valid(&key));
    }

    #[test]
    fn a_thousand_inserts_at_the_front_stay_ordered() {
        let mut first = key_between(None, None);
        for _ in 0..1_000 {
            let next = key_between(None, Some(&first));
            assert!(next < first && valid(&next));
            first = next;
        }
    }

    #[test]
    fn even_keys_are_ordered_and_short() {
        for count in [0, 1, 61, 62, 63, 3_844, 5_000] {
            let keys = even_keys(count);
            assert_eq!(keys.len(), count);
            assert!(keys.windows(2).all(|w| w[0] < w[1]));
            assert!(keys.iter().all(|key| valid(key) && key.len() <= 4));
        }
    }

    proptest! {
        /// Any sequence of inserts keeps the list strictly ordered with valid keys.
        #[test]
        fn inserts_keep_strict_order(slots in prop::collection::vec(any::<prop::sample::Index>(), 1..200)) {
            let mut keys: Vec<String> = Vec::new();
            for slot in slots {
                let at = slot.index(keys.len() + 1);
                let before = at.checked_sub(1).and_then(|i| keys.get(i)).map(String::as_str);
                let after = keys.get(at).map(String::as_str);
                let key = key_between(before, after);
                prop_assert!(valid(&key), "invalid key {key}");
                prop_assert!(before.is_none_or(|b| b < key.as_str()));
                prop_assert!(after.is_none_or(|a| key.as_str() < a));
                keys.insert(at, key);
            }
            prop_assert!(keys.windows(2).all(|w| w[0] < w[1]));
        }
    }
}
