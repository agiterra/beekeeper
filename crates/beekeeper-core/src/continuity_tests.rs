use super::*;
use nostr::{Event, EventBuilder, Keys, Kind, Tag, Timestamp};

const CHANNEL: &str = "12345678-1234-1234-1234-123456789abc";
const BASE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
fn scope() -> Scope {
    Scope::new("https://hive.agiterra.org", CHANNEL, None).unwrap()
}
fn keys(n: u8) -> Keys {
    Keys::parse(&format!("{n:064x}")).unwrap()
}
fn claim(id: &str) -> Claim {
    Claim {
        id: id.into(),
        state: ClaimState::Asserted,
        text: "observed A".into(),
        observed_at: 10,
        evidence: vec![],
        correction: None,
    }
}
fn record() -> Record {
    Record {
        schema: SCHEMA.into(),
        scope: scope(),
        base_event_id: None,
        claims: vec![claim("deployment")],
    }
}
fn value(record: &Record) -> String {
    serde_json::to_string(record).unwrap()
}
fn event(record: &Record, timestamp: u64) -> Event {
    crate::engram::build_event(
        &keys(1),
        &keys(2).public_key(),
        &record.to_body().unwrap(),
        timestamp,
    )
    .unwrap()
}
fn raw_event(value: Option<String>, timestamp: u64) -> Event {
    crate::engram::build_event(
        &keys(1),
        &keys(2).public_key(),
        &Body::Memory {
            slug: scope().slug().unwrap(),
            value,
        },
        timestamp,
    )
    .unwrap()
}
fn read(events: &[Event], coverage: QueryCoverage) -> HeadRead {
    let a = keys(1);
    let o = keys(2);
    select_head(
        &scope(),
        &ReadIdentity {
            actor: &a.public_key(),
            owner: &o.public_key(),
            reader_secret: a.secret_key(),
            peer: &o.public_key(),
        },
        events,
        coverage,
    )
}
fn head(events: &[Event]) -> Box<Head> {
    let HeadRead::Present { head } = read(events, QueryCoverage::Complete) else {
        panic!("expected valid head");
    };
    head
}
fn corrected(previous: &Record, prior: &str) -> Record {
    let mut next = previous.clone();
    next.base_event_id = Some(prior.into());
    next.claims[0].text = "observed B".into();
    next.claims[0].correction = Some(Correction {
        prior_head: prior.into(),
        prior_text: previous.claims[0].text.clone(),
        reason: "new observation".into(),
    });
    next
}
fn source() -> Event {
    EventBuilder::new(Kind::Custom(9), "report: observed B 🐝")
        .tags([Tag::parse(["h", CHANNEL]).unwrap()])
        .custom_created_at(Timestamp::from(12))
        .sign_with_keys(&keys(3))
        .unwrap()
}
fn evidence(event: &Event) -> Evidence {
    Evidence {
        kind: event.kind.as_u16(),
        event_id: event.id.to_hex(),
        channel: CHANNEL.into(),
        sha256: value_hash(event.content.as_bytes()),
        excerpt: "observed B 🐝".into(),
    }
}

#[test]
fn canonical_scope_and_slug_vector() {
    let s = Scope::new(
        "WSS://HIVE.AGITERRA.ORG:443/",
        &CHANNEL.to_uppercase(),
        None,
    )
    .unwrap();
    assert_eq!(s, scope());
    assert_eq!(
        s.slug().unwrap(),
        format!(
            "mem/continuity/v1/{}",
            value_hash(format!("[\"https://hive.agiterra.org\",\"{CHANNEL}\",null]").as_bytes())
        )
    );
    assert_eq!(
        canonical_origin("ws://LOCALHOST:80").unwrap(),
        "http://localhost"
    );
    assert_eq!(
        canonical_origin("wss://[::1]:444/").unwrap(),
        "https://[::1]:444"
    );
    for bad in [
        "https://hive.agiterra.org/sub",
        "https://user@hive.agiterra.org/",
        "https://hive.agiterra.org/?a=b",
        "https://hive.agiterra.org/#x",
        "ftp://hive.agiterra.org",
        "not a URL",
    ] {
        assert!(canonical_origin(bad).is_err(), "{bad}");
    }
    let mut noncanonical = scope();
    noncanonical.community.push('/');
    assert_eq!(noncanonical.slug(), Err(Error::Scope));
    let mut other = scope();
    other.session = Some(CHANNEL.into());
    assert_ne!(other.slug().unwrap(), scope().slug().unwrap());
    assert!(Record::decode(&value(&record()), &other).is_err());
}

#[test]
fn strict_json_requires_null_fields_and_rejects_duplicates_at_all_levels() {
    let mut r = corrected(&record(), BASE);
    r.claims[0].evidence.push(evidence(&source()));
    let json = value(&r);
    assert_eq!(Record::decode(&json, &scope()).unwrap(), r);
    for key in ["schema", "community", "id", "kind", "reason"] {
        let needle = format!("\"{key}\":");
        let broken = json.replacen(&needle, &format!("\"{key}\":null,{needle}"), 1);
        assert!(Record::decode(&broken, &scope()).is_err(), "{key}");
    }
    for needle in [
        "\"schema\":",
        "\"community\":",
        "\"id\":",
        "\"kind\":",
        "\"reason\":",
    ] {
        assert!(Record::decode(
            &json.replacen(needle, &format!("\"surprise\":0,{needle}"), 1),
            &scope()
        )
        .is_err());
    }
    let json = value(&record());
    for field in [
        ",\"session\":null",
        "\"base_event_id\":null,",
        ",\"correction\":null",
    ] {
        assert!(
            Record::decode(&json.replace(field, ""), &scope()).is_err(),
            "{field}"
        );
    }
    assert!(Record::decode(&json.replace(SCHEMA, "beekeeper-continuity/2"), &scope()).is_err());
    assert!(Record::decode(
        &json.replace("\"observed_at\":10", "\"observed_at\":-1"),
        &scope()
    )
    .is_err());
    assert!(Record::decode(&format!("{json} false"), &scope()).is_err());
}

#[test]
fn identifiers_and_array_bounds() {
    for id in ["", "A", "-a", "a/b", "a.b", "é"] {
        assert!(validate_claim_id(id).is_err());
    }
    assert!(validate_claim_id(&"a".repeat(64)).is_ok());
    assert!(validate_claim_id(&"a".repeat(65)).is_err());
    let mut r = record();
    r.claims.push(r.claims[0].clone());
    assert_eq!(r.to_body(), Err(Error::Claim));
    r.claims = (0..16).map(|n| claim(&format!("c{n}"))).collect();
    assert!(r.to_body().is_ok());
    r.claims.push(claim("c16"));
    assert_eq!(r.to_body(), Err(Error::TooLarge));
    r = record();
    r.claims[0].evidence = vec![evidence(&source()); 4];
    assert!(r.to_body().is_ok());
    r.claims[0].evidence.push(evidence(&source()));
    assert_eq!(r.to_body(), Err(Error::TooLarge));
}

#[test]
fn utf8_and_nested_escape_bounds_never_trim() {
    let mut r = record();
    r.claims[0].text = "🐝".repeat(512);
    assert!(r.to_body().is_ok());
    r.claims[0].text.push('a');
    assert_eq!(r.to_body(), Err(Error::TooLarge));
    r.claims = (0..16)
        .map(|n| {
            let mut c = claim(&format!("c{n}"));
            c.text = "\n".repeat(2048);
            c
        })
        .collect();
    assert!(value(&r).len() < crate::engram::NIP44_PLAINTEXT_MAX + 5000);
    assert_eq!(r.to_body(), Err(Error::TooLarge));
    // Exact outer boundary with JSON whitespace: each space is one outer byte.
    let r = record();
    let raw = value(&r);
    let base_len = Body::Memory {
        slug: scope().slug().unwrap(),
        value: Some(raw.clone()),
    }
    .to_json_bytes()
    .len();
    let at_limit = format!("{}{}", raw, " ".repeat(MAX_BODY_BYTES - base_len));
    assert!(Record::decode(&at_limit, &scope()).is_ok());
    assert_eq!(
        Record::decode(&format!("{at_limit} "), &scope()),
        Err(Error::TooLarge)
    );
    let mut r = corrected(&record(), BASE);
    r.claims[0].correction.as_mut().unwrap().reason = "é".repeat(1025);
    assert_eq!(r.to_body(), Err(Error::TooLarge));
    r = record();
    let mut e = evidence(&source());
    e.excerpt = "🐝".repeat(513);
    r.claims[0].evidence.push(e);
    assert_eq!(r.to_body(), Err(Error::TooLarge));
}

#[test]
fn write_sorts_but_stored_hash_is_exact_bytes() {
    let mut r = record();
    r.claims.push(claim("aaa"));
    let Body::Memory {
        value: Some(encoded),
        ..
    } = r.to_body().unwrap()
    else {
        panic!()
    };
    assert_eq!(
        Record::decode(&encoded, &scope()).unwrap().claims[0].id,
        "aaa"
    );
    let raw = format!(" {} ", value(&r));
    let selected = head(&[raw_event(Some(raw.clone()), 100)]);
    assert_eq!(selected.value, raw);
    assert_eq!(selected.value_hash, value_hash(raw.as_bytes()));
    assert_ne!(selected.value_hash, value_hash(encoded.as_bytes()));
}

#[test]
fn corrections_require_immediate_base_and_preserve_all_prior_claims() {
    let original = record();
    assert!(validate_transition(&original, None).is_ok());
    let next = corrected(&original, BASE);
    assert!(validate_transition(&next, Some((BASE, &original))).is_ok());
    assert_eq!(validate_transition(&next, None), Err(Error::BaseMismatch));
    let mut bad = next.clone();
    bad.claims[0].correction = None;
    assert_eq!(
        validate_transition(&bad, Some((BASE, &original))),
        Err(Error::Transition)
    );
    bad = next.clone();
    bad.claims.clear();
    assert_eq!(
        validate_transition(&bad, Some((BASE, &original))),
        Err(Error::Transition)
    );
    bad = next.clone();
    bad.claims[0]
        .correction
        .as_mut()
        .unwrap()
        .prior_text
        .push('!');
    assert_eq!(
        validate_transition(&bad, Some((BASE, &original))),
        Err(Error::Transition)
    );
    bad = next.clone();
    bad.claims[0].correction.as_mut().unwrap().prior_head = "b".repeat(64);
    assert_eq!(
        validate_transition(&bad, Some((BASE, &original))),
        Err(Error::Transition)
    );
    bad = next.clone();
    bad.claims[0].correction.as_mut().unwrap().reason = " ".into();
    assert!(validate_transition(&bad, Some((BASE, &original))).is_err());
    // Unchanged claims retain their older correction, not the new head's base.
    let new_base = "b".repeat(64);
    let mut added = next.clone();
    added.base_event_id = Some(new_base.clone());
    added.claims.push(claim("new"));
    assert!(validate_transition(&added, Some((&new_base, &next))).is_ok());
    added.claims[1].correction = next.claims[0].correction.clone();
    assert_eq!(
        validate_transition(&added, Some((&new_base, &next))),
        Err(Error::Transition)
    );
}

#[test]
fn withdrawal_and_restoration_are_explicit_corrections() {
    let r = record();
    let mut withdrawn = corrected(&r, BASE);
    withdrawn.claims[0].state = ClaimState::Withdrawn;
    assert_eq!(
        validate_transition(&withdrawn, Some((BASE, &r))),
        Err(Error::Transition)
    );
    withdrawn.claims[0].text = r.claims[0].text.clone();
    assert!(validate_transition(&withdrawn, Some((BASE, &r))).is_ok());
    let h = head(&[event(&withdrawn, 20)]);
    assert!(matches!(
        &evaluate_claims(&h, &[], |_| panic!("withdrawal must not fetch")).unwrap()[0],
        ClaimOutcome::Withdrawn { .. }
    ));
    let mut restored = corrected(&withdrawn, BASE);
    restored.claims[0].state = ClaimState::Asserted;
    assert!(validate_transition(&restored, Some((BASE, &withdrawn))).is_ok());
    restored.claims[0].correction = None;
    assert_eq!(
        validate_transition(&restored, Some((BASE, &withdrawn))),
        Err(Error::Transition)
    );
}

#[test]
fn failed_or_capped_read_never_means_absent() {
    assert_eq!(read(&[], QueryCoverage::Complete), HeadRead::AbsentAtRead);
    assert_eq!(
        read(&[], QueryCoverage::Failed),
        HeadRead::Unknown {
            reason: UnknownReason::QueryFailed
        }
    );
    assert_eq!(
        read(&[], QueryCoverage::Incomplete),
        HeadRead::Unknown {
            reason: UnknownReason::Incomplete
        }
    );
    assert_eq!(
        read(&vec![event(&record(), 1); 16], QueryCoverage::Complete),
        HeadRead::Unknown {
            reason: UnknownReason::Incomplete
        }
    );
}

#[test]
fn malformed_newest_head_shadows_valid_old_and_tombstone_is_not_absence() {
    let old = event(&record(), 1);
    let malformed = raw_event(Some("{}".into()), 2);
    assert_eq!(
        read(&[old.clone(), malformed], QueryCoverage::Complete),
        HeadRead::Unknown {
            reason: UnknownReason::InvalidHead
        }
    );
    let tombstone = raw_event(None, 3);
    assert_eq!(
        read(&[old, tombstone.clone()], QueryCoverage::Complete),
        HeadRead::WithdrawnRecord {
            event_id: tombstone.id.to_hex()
        }
    );
}

#[test]
fn signature_and_identity_checked_before_decrypt() {
    let old = event(&record(), 1);
    let mut forged = old.clone();
    forged.content.push('x');
    assert_eq!(
        read(&[old.clone(), forged], QueryCoverage::Complete),
        HeadRead::Unknown {
            reason: UnknownReason::InvalidCandidate
        }
    );
    let a = keys(1);
    let wrong = keys(3);
    let identity = ReadIdentity {
        actor: &a.public_key(),
        owner: &wrong.public_key(),
        reader_secret: a.secret_key(),
        peer: &wrong.public_key(),
    };
    assert_eq!(
        select_head(&scope(), &identity, &[old], QueryCoverage::Complete),
        HeadRead::Unknown {
            reason: UnknownReason::InvalidCandidate
        }
    );
    let mut other = record();
    other.scope.community = "https://other.example".into();
    assert_eq!(
        read(&[event(&other, 1)], QueryCoverage::Complete),
        HeadRead::Unknown {
            reason: UnknownReason::InvalidCandidate
        }
    );
}

#[test]
fn visible_fork_marks_only_differing_claims_unknown_including_loser_only_ids() {
    let mut a = corrected(&record(), BASE);
    a.claims.push(claim("unchanged"));
    let mut b = a.clone();
    b.claims[0].text = "different B".into();
    b.claims.push(claim("loser-only"));
    let winner = event(&a, 30);
    let loser = event(&b, 29);
    let h = head(&[winner.clone(), loser]);
    assert_eq!(h.event_id, winner.id.to_hex());
    assert_eq!(
        h.conflicts,
        BTreeSet::from(["deployment".into(), "loser-only".into()])
    );
    let outcomes = evaluate_claims(&h, &[], |_| panic!()).unwrap();
    assert_eq!(outcomes.len(), 3);
    assert!(
        matches!(&outcomes[0], ClaimOutcome::Unknown { reasons, .. } if reasons == &[UnknownReason::Conflict])
    );
    assert!(
        matches!(&outcomes[1], ClaimOutcome::Unknown { reasons, .. } if reasons == &[UnknownReason::Conflict])
    );
    assert!(
        matches!(&outcomes[2], ClaimOutcome::Unknown { reasons, .. } if reasons == &[UnknownReason::NoEvidence])
    );
    // A later explicit correction acknowledges the selected head; no merge.
    let resolved = corrected(&a, &winner.id.to_hex());
    assert!(head(&[winner, event(&resolved, 31)]).conflicts.is_empty());
}

#[test]
fn nip01_lowest_id_tie_and_identical_same_base_writes_do_not_conflict() {
    let a = event(&record(), 10);
    let b = event(&record(), 10);
    let expected = a.id.to_hex().min(b.id.to_hex());
    let h = head(&[a, b]);
    assert_eq!(h.event_id, expected);
    assert!(h.conflicts.is_empty());
}

#[test]
fn correction_survives_discarded_old_head_and_sources_are_not_truth() {
    let source = source();
    let mut corrected = corrected(&record(), BASE);
    corrected.claims[0].evidence.push(evidence(&source));
    let h = head(&[event(&corrected, 12)]);
    let outcomes = evaluate_claims(&h, &[], |_| Ok(source.clone())).unwrap();
    let ClaimOutcome::AssertionWithReadableEvidence {
        author,
        claim,
        head_event_id,
    } = &outcomes[0]
    else {
        panic!()
    };
    assert_eq!(author, &keys(1).public_key().to_hex());
    assert_eq!(head_event_id, &h.event_id);
    assert_eq!(claim.text, "observed B");
    assert_eq!(claim.correction.as_ref().unwrap().prior_text, "observed A");
    assert_eq!(claim.observed_at, 10);
    let missing = evaluate_claims(&h, &[], |_| Err(UnknownReason::EvidenceUnavailable)).unwrap();
    assert!(
        matches!(&missing[0], ClaimOutcome::Unknown { reasons, unresolved_ids, .. } if reasons == &[UnknownReason::EvidenceUnavailable] && unresolved_ids == &[source.id.to_hex()])
    );
    let missing_id = evaluate_claims(&h, &["not-present".into()], |_| panic!()).unwrap();
    assert!(
        matches!(&missing_id[0], ClaimOutcome::Unknown { reasons, .. } if reasons == &[UnknownReason::NotFound])
    );
}

#[test]
fn every_source_must_resolve_and_exact_content_and_scope_are_verified() {
    let source = source();
    let e = evidence(&source);
    assert!(verify_evidence(&e, &source).is_ok());
    let mut bad = e.clone();
    bad.sha256 = "0".repeat(64);
    assert_eq!(
        verify_evidence(&bad, &source),
        Err(UnknownReason::EvidenceHash)
    );
    bad = e.clone();
    bad.excerpt = "not a substring".into();
    assert_eq!(
        verify_evidence(&bad, &source),
        Err(UnknownReason::EvidenceExcerpt)
    );
    bad = e.clone();
    bad.kind = 10;
    assert_eq!(
        verify_evidence(&bad, &source),
        Err(UnknownReason::EvidenceScope)
    );
    bad = e.clone();
    bad.channel = "00000000-0000-0000-0000-000000000000".into();
    assert_eq!(
        verify_evidence(&bad, &source),
        Err(UnknownReason::EvidenceScope)
    );
    bad = e.clone();
    bad.event_id = "0".repeat(64);
    assert_eq!(
        verify_evidence(&bad, &source),
        Err(UnknownReason::EvidenceScope)
    );
    let mut forged = source.clone();
    forged.content.push('!');
    assert_eq!(
        verify_evidence(&e, &forged),
        Err(UnknownReason::EvidenceInvalid)
    );
    let mut r = record();
    r.claims[0].evidence = vec![e, bad];
    let h = head(&[event(&r, 10)]);
    let mut calls = 0;
    let outcomes = evaluate_claims(&h, &[], |_| {
        calls += 1;
        Ok(source.clone())
    })
    .unwrap();
    assert_eq!(calls, 2);
    assert!(matches!(&outcomes[0], ClaimOutcome::Unknown { .. }));
}

#[test]
fn clock_poison_is_refused_without_saturation_success() {
    assert_eq!(write_created_at(100, None), Ok(100));
    assert_eq!(write_created_at(100, Some(159)), Ok(160));
    assert_eq!(write_created_at(100, Some(160)), Err(Error::Clock));
    assert_eq!(
        write_created_at(u64::MAX, Some(u64::MAX)),
        Err(Error::Clock)
    );
}

#[test]
fn visible_predecessor_detects_invalid_correction_without_fallback() {
    let previous = record();
    let old = event(&previous, 1);
    let mut invalid = previous.clone();
    invalid.base_event_id = Some(old.id.to_hex());
    invalid.claims[0].text = "changed without correction".into();
    assert_eq!(
        read(&[old, event(&invalid, 2)], QueryCoverage::Complete),
        HeadRead::Unknown {
            reason: UnknownReason::InvalidHead
        }
    );
}

#[test]
fn signed_oversize_outer_plaintext_is_unknown_even_with_small_valid_value() {
    let agent = keys(1);
    let owner = keys(2);
    let body = record().to_body().unwrap();
    let mut plaintext = String::from_utf8(body.to_json_bytes()).unwrap();
    plaintext.push_str(&" ".repeat(MAX_BODY_BYTES + 1 - plaintext.len()));
    let encrypted = nostr::nips::nip44::encrypt(
        agent.secret_key(),
        &owner.public_key(),
        plaintext,
        nostr::nips::nip44::Version::V2,
    )
    .unwrap();
    let normal = event(&record(), 1);
    let oversized = EventBuilder::new(Kind::Custom(30174), encrypted)
        .tags(normal.tags.clone())
        .custom_created_at(Timestamp::from(2))
        .sign_with_keys(&agent)
        .unwrap();
    assert_eq!(
        read(&[normal, oversized], QueryCoverage::Complete),
        HeadRead::Unknown {
            reason: UnknownReason::InvalidHead
        }
    );
}

#[test]
fn url_parser_cannot_erase_nonroot_paths() {
    for endpoint in [
        "https://hive.agiterra.org/a/..",
        "https://hive.agiterra.org/./",
        " https://hive.agiterra.org/",
        "https://hive.agiterra.org\\",
    ] {
        assert_eq!(canonical_origin(endpoint), Err(Error::Scope), "{endpoint}");
    }
}

#[test]
fn unrelated_descendant_does_not_hide_visible_fork_but_explicit_correction_resolves() {
    let a = corrected(&record(), BASE);
    let mut b = a.clone();
    b.claims[0].text = "fork B".into();
    let left = event(&a, 20);
    let right = event(&b, 21);
    let mut unrelated = b.clone();
    unrelated.base_event_id = Some(right.id.to_hex());
    unrelated.claims.push(claim("other"));
    let selected = head(&[left.clone(), right.clone(), event(&unrelated, 22)]);
    assert!(selected.conflicts.contains("deployment"));
    let resolved = corrected(&b, &right.id.to_hex());
    let selected = head(&[left, right, event(&resolved, 23)]);
    assert!(selected.conflicts.is_empty());
}
