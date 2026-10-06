//! Fractional ranks for ordered, concurrently edited lists.
//!
//! A rank is a base-62 string that compares as plain bytes — the alphabet is
//! in ASCII order — and between any two ranks another can always be minted,
//! so a client moving one item never rewrites its neighbours. That is what
//! makes reordering safe under the per-field last-write-wins fold of
//! `project_todo_fold`: a move is one `item.rank` op on one item.
//!
//! The scheme is the widely used "fractional indexing" order key (rocicorp's
//! `generateKeyBetween`): an **integer part** whose first character encodes
//! its length (`a`+1 digit, `b`+2 digits … for non-negative; `Z`+1 digit,
//! `Y`+2 digits … for negative), followed by an optional **fraction** of
//! base-62 digits. Appending to the end of a list increments the integer
//! (`a0`, `a1`, … `a9`, `aA`, …), so a list of thousands of appended items
//! keeps two- or three-character ranks; inserting between two neighbours
//! takes the string midpoint of their fractions and grows by one digit only
//! when the gap is exhausted.
//!
//! One rule is stated here so every implementation agrees bit for bit: the
//! midpoint digit between fraction digits `a` and `b` is `(a + b + 1) / 2`
//! in integer arithmetic. The TypeScript and Dart twins are pinned to the
//! same vectors in `conformance/project-todo-fold/fixtures/rank-vectors.json`;
//! a rank one client mints must sort identically everywhere.
//!
//! Invariants [`validate_rank`] enforces: the head is a letter, the integer
//! part is exactly as long as the head says, every byte is a digit, the
//! fraction never ends in the smallest digit `0` (it would add nothing but
//! make `"a0V"` and `"a0V0"` compare unequal), and the whole rank is at most
//! [`MAX_RANK_LEN`] bytes.

/// The digit alphabet, in ASCII order so byte comparison is numeric order.
pub const RANK_DIGITS: &[u8; 62] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Maximum length of a rank. Inserting between the same two neighbours grows
/// the fraction by one digit per insertion, so this is the depth a client
/// can go before it must re-rank the list.
pub const MAX_RANK_LEN: usize = 64;

/// The first rank of an empty list.
pub const FIRST_RANK: &str = "a0";

fn digit_value(byte: u8) -> Option<usize> {
    RANK_DIGITS.iter().position(|d| *d == byte)
}

/// How many bytes the integer part is, given its head letter.
fn integer_length(head: u8) -> Result<usize, String> {
    match head {
        b'a'..=b'z' => Ok((head - b'a') as usize + 2),
        b'A'..=b'Z' => Ok((b'Z' - head) as usize + 2),
        other => Err(format!(
            "rank must start with a letter (got {:?})",
            other as char
        )),
    }
}

/// Split a rank into `(integer, fraction)`, validating the integer length.
fn split_rank(rank: &[u8]) -> Result<(&[u8], &[u8]), String> {
    let head = *rank
        .first()
        .ok_or_else(|| "rank must not be empty".to_owned())?;
    let len = integer_length(head)?;
    if rank.len() < len {
        return Err("rank integer part is shorter than its head declares".to_owned());
    }
    Ok(rank.split_at(len))
}

/// Check that `rank` is a well-formed rank (see the module doc).
pub fn validate_rank(rank: &str) -> Result<(), String> {
    let bytes = rank.as_bytes();
    if bytes.len() > MAX_RANK_LEN {
        return Err(format!("rank exceeds {MAX_RANK_LEN} characters"));
    }
    let (integer, fraction) = split_rank(bytes)?;
    if let Some(bad) = integer[1..]
        .iter()
        .chain(fraction.iter())
        .find(|b| digit_value(**b).is_none())
    {
        return Err(format!(
            "rank contains a character outside 0-9A-Za-z ({:?})",
            *bad as char
        ));
    }
    if fraction.last() == Some(&b'0') {
        return Err("rank fraction must not end in 0".to_owned());
    }
    if integer == smallest_integer().as_slice() && fraction.is_empty() {
        return Err("rank is below the smallest representable order".to_owned());
    }
    Ok(())
}

/// `A` followed by 26 zeros: the smallest integer part, reserved as the
/// floor below which nothing can be minted.
fn smallest_integer() -> Vec<u8> {
    let mut v = vec![b'A'];
    v.extend(std::iter::repeat_n(b'0', 26));
    v
}

/// The string midpoint of fractions `a` (possibly empty, meaning zero) and
/// `b` (`None` meaning one). Precondition: `a < b`, neither ends in `0`.
fn midpoint(a: &[u8], b: Option<&[u8]>) -> Vec<u8> {
    if let Some(b) = b {
        // Peel the longest common prefix; a missing digit of `a` reads as
        // `0`, which is what a shorter `a < b` means.
        let mut n = 0;
        while n < b.len() && a.get(n).copied().unwrap_or(b'0') == b[n] {
            n += 1;
        }
        if n > 0 {
            let mut out = b[..n].to_vec();
            out.extend(midpoint(&a[n.min(a.len())..], Some(&b[n..])));
            return out;
        }
    }
    let digit_a = a.first().and_then(|d| digit_value(*d)).unwrap_or(0);
    let digit_b = b
        .and_then(|b| b.first())
        .and_then(|d| digit_value(*d))
        .unwrap_or(RANK_DIGITS.len());
    if digit_b - digit_a > 1 {
        let mid = (digit_a + digit_b).div_ceil(2);
        return vec![RANK_DIGITS[mid]];
    }
    // The first digits are adjacent. If `b` has more digits, its first digit
    // alone already sits strictly between (`b`'s tail is positive because it
    // never ends in 0). Otherwise descend into `a`'s tail against "one".
    match b {
        Some(b) if b.len() > 1 => vec![b[0]],
        _ => {
            let mut out = vec![RANK_DIGITS[digit_a]];
            out.extend(midpoint(a.get(1..).unwrap_or(&[]), None));
            out
        }
    }
}

/// The next integer part after `x`, or `None` at the top of the range.
fn increment_integer(x: &[u8]) -> Option<Vec<u8>> {
    let head = x[0];
    let mut digits = x[1..].to_vec();
    let mut carry = true;
    for d in digits.iter_mut().rev() {
        if !carry {
            break;
        }
        let v = digit_value(*d)? + 1;
        if v == RANK_DIGITS.len() {
            *d = b'0';
        } else {
            *d = RANK_DIGITS[v];
            carry = false;
        }
    }
    if !carry {
        let mut out = vec![head];
        out.extend(digits);
        return Some(out);
    }
    match head {
        b'Z' => Some(b"a0".to_vec()),
        b'z' => None,
        _ => {
            let next = head + 1;
            if next > b'a' {
                digits.push(b'0');
            } else {
                digits.pop();
            }
            let mut out = vec![next];
            out.extend(digits);
            Some(out)
        }
    }
}

/// The previous integer part before `x`, or `None` at the bottom of the
/// range.
fn decrement_integer(x: &[u8]) -> Option<Vec<u8>> {
    let head = x[0];
    let mut digits = x[1..].to_vec();
    let last = RANK_DIGITS[RANK_DIGITS.len() - 1];
    let mut borrow = true;
    for d in digits.iter_mut().rev() {
        if !borrow {
            break;
        }
        match digit_value(*d)?.checked_sub(1) {
            None => *d = last,
            Some(v) => {
                *d = RANK_DIGITS[v];
                borrow = false;
            }
        }
    }
    if !borrow {
        let mut out = vec![head];
        out.extend(digits);
        return Some(out);
    }
    match head {
        b'a' => Some(vec![b'Z', last]),
        b'A' => None,
        _ => {
            let prev = head - 1;
            if prev < b'Z' {
                digits.push(last);
            } else {
                digits.pop();
            }
            let mut out = vec![prev];
            out.extend(digits);
            Some(out)
        }
    }
}

/// Mint a rank strictly between `after` and `before`.
///
/// `None` on the left means "before everything" and `None` on the right
/// means "after everything"; `rank_between(None, None)` is [`FIRST_RANK`].
/// Both bounds must pass [`validate_rank`] and `after < before` must hold,
/// otherwise an error names the problem — a caller that computed its
/// neighbours from a stale fold gets told rather than handed a rank that
/// sorts somewhere else.
pub fn rank_between(after: Option<&str>, before: Option<&str>) -> Result<String, String> {
    if let Some(a) = after {
        validate_rank(a)?;
    }
    if let Some(b) = before {
        validate_rank(b)?;
    }
    if let (Some(a), Some(b)) = (after, before) {
        if a >= b {
            return Err(format!("rank bounds are not ordered ({a:?} >= {b:?})"));
        }
    }
    let minted: Vec<u8> = match (after.map(str::as_bytes), before.map(str::as_bytes)) {
        (None, None) => FIRST_RANK.as_bytes().to_vec(),
        (None, Some(b)) => {
            let (ib, fb) = split_rank(b)?;
            if ib == smallest_integer().as_slice() {
                let mut out = ib.to_vec();
                out.extend(midpoint(&[], Some(fb)));
                out
            } else if !fb.is_empty() {
                ib.to_vec()
            } else {
                decrement_integer(ib)
                    .ok_or_else(|| "rank cannot go below the smallest order".to_owned())?
            }
        }
        (Some(a), None) => {
            let (ia, fa) = split_rank(a)?;
            match increment_integer(ia) {
                Some(next) => next,
                None => {
                    let mut out = ia.to_vec();
                    out.extend(midpoint(fa, None));
                    out
                }
            }
        }
        (Some(a), Some(b)) => {
            let (ia, fa) = split_rank(a)?;
            let (ib, fb) = split_rank(b)?;
            if ia == ib {
                let mut out = ia.to_vec();
                out.extend(midpoint(fa, Some(fb)));
                out
            } else {
                let next = increment_integer(ia)
                    .ok_or_else(|| "rank cannot go above the largest order".to_owned())?;
                if next.as_slice() < b {
                    next
                } else {
                    let mut out = ia.to_vec();
                    out.extend(midpoint(fa, None));
                    out
                }
            }
        }
    };
    let minted = String::from_utf8(minted).map_err(|_| "rank is not ASCII".to_owned())?;
    if minted.len() > MAX_RANK_LEN {
        return Err(format!(
            "rank between {after:?} and {before:?} would exceed {MAX_RANK_LEN} characters; re-rank the list"
        ));
    }
    Ok(minted)
}

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
