//! Fractional ranks — moved to [`beekeeper_nip_td::fractional_rank`] so a
//! WebAssembly plugin can link them; re-exported here unchanged.

pub use beekeeper_nip_td::fractional_rank::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_rank_of_an_empty_list() {
        assert_eq!(rank_between(None, None).unwrap(), "a0");
    }

    #[test]
    fn minted_ranks_sort_between_their_bounds() {
        let cases: &[(Option<&str>, Option<&str>)] = &[
            (None, None),
            (Some("a0"), None),
            (None, Some("a0")),
            (Some("a0"), Some("a1")),
            (Some("a0"), Some("a0V")),
            (Some("a0V"), Some("a1")),
            (Some("a0"), Some("a2")),
            (Some("Zz"), Some("a0")),
            (None, Some("Zz")),
            (Some("a0Vzz"), Some("a1")),
            (Some("b00"), None),
            (Some("zzzzzzzzzzzzzzzzzzzzzzzzzzz"), None),
        ];
        for (after, before) in cases {
            let minted = rank_between(*after, *before).unwrap();
            validate_rank(&minted).unwrap();
            if let Some(a) = after {
                assert!(*a < minted.as_str(), "{after:?} < {minted}");
            }
            if let Some(b) = before {
                assert!(minted.as_str() < *b, "{minted} < {before:?}");
            }
        }
    }

    #[test]
    fn appends_increment_the_integer_and_stay_short() {
        let mut ranks: Vec<String> = Vec::new();
        for _ in 0..1_000 {
            let next = rank_between(ranks.last().map(String::as_str), None).unwrap();
            ranks.push(next);
        }
        assert!(ranks.windows(2).all(|w| w[0] < w[1]));
        assert!(ranks.iter().all(|r| r.len() <= 3), "{:?}", &ranks[990..]);
        assert_eq!(&ranks[..3], &["a0", "a1", "a2"]);
        assert_eq!(ranks[62], "b00");

        let mut head = rank_between(None, None).unwrap();
        for _ in 0..200 {
            let before = rank_between(None, Some(&head)).unwrap();
            assert!(before < head);
            head = before;
        }
        assert!(head.len() <= 3);
    }

    #[test]
    fn inserting_at_one_gap_grows_by_one_digit_each_time() {
        let mut lo = "a0".to_owned();
        let hi = "a1".to_owned();
        for depth in 1..=20 {
            lo = rank_between(Some(&lo), Some(&hi)).unwrap();
            assert!(lo < hi);
            assert!(lo.len() <= 2 + depth);
        }
    }

    #[test]
    fn rejects_malformed_bounds() {
        assert!(rank_between(Some("a1"), Some("a0")).is_err());
        assert!(rank_between(Some("a0"), Some("a0")).is_err());
        assert!(rank_between(Some("a0V0"), None).is_err());
        assert!(rank_between(Some(""), None).is_err());
        assert!(rank_between(Some("0"), None).is_err());
        assert!(rank_between(Some("a"), None).is_err());
        assert!(rank_between(Some("a0-"), None).is_err());
        assert!(validate_rank(&format!("a0{}", "z".repeat(MAX_RANK_LEN))).is_err());
        assert!(validate_rank(&format!("a0{}", "z".repeat(MAX_RANK_LEN - 2))).is_ok());
    }

    #[test]
    fn pinned_examples() {
        assert_eq!(rank_between(Some("a0"), Some("a1")).unwrap(), "a0V");
        assert_eq!(rank_between(Some("a0"), Some("a0V")).unwrap(), "a0G");
        assert_eq!(rank_between(Some("a0V"), Some("a1")).unwrap(), "a0l");
        assert_eq!(rank_between(Some("a03"), Some("a06")).unwrap(), "a05");
        assert_eq!(rank_between(Some("a03"), Some("a04")).unwrap(), "a03V");
        assert_eq!(rank_between(None, Some("a0")).unwrap(), "Zz");
        assert_eq!(rank_between(Some("az"), None).unwrap(), "b00");
        assert_eq!(rank_between(Some("a0"), Some("a2")).unwrap(), "a1");
    }
}
