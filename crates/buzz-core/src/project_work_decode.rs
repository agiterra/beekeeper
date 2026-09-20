//! Decoding, envelope validation and the field validators for kind 44249.
//!
//! A child of `project_work`, split out only to keep every file under 1,000
//! lines, exactly as kind 44244's decoder is. No behaviour lives here that
//! the contract does not name: `use super::*` gives these functions the
//! parent's types, constants and bounds unchanged.

use super::*;

type Refused<T> = Result<T, ProjectWorkRefusal>;

fn refuse<T>(
    code: ProjectWorkRefusalCode,
    path: impl Into<String>,
    message: impl Into<String>,
) -> Refused<T> {
    Err(ProjectWorkRefusal::new(code, path, message))
}

/// Whether `value` is lowercase hex of one of the accepted lengths.
fn is_hex(value: &str, lengths: &[usize]) -> bool {
    lengths.contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn check_event_id(value: &str, path: &str) -> Refused<()> {
    if is_hex(value, &[64]) {
        Ok(())
    } else {
        refuse(
            ProjectWorkRefusalCode::Hex,
            path,
            format!("{path} is a 64-character lowercase hex event id"),
        )
    }
}

fn check_commit(value: &str, path: &str) -> Refused<()> {
    if is_hex(value, &[40, 64]) {
        Ok(())
    } else {
        refuse(
            ProjectWorkRefusalCode::Hex,
            path,
            format!("{path} is a full 40- or 64-character lowercase hex commit"),
        )
    }
}

fn check_uuid(value: &str, path: &str) -> Refused<()> {
    let canonical = uuid::Uuid::parse_str(value)
        .ok()
        .map(|parsed| parsed.to_string());
    if canonical.as_deref() == Some(value) {
        Ok(())
    } else {
        refuse(
            ProjectWorkRefusalCode::Uuid,
            path,
            format!("{path} is a canonical lowercase hyphenated uuid"),
        )
    }
}

/// Whether `value` is a canonical `30621:<64 lowercase hex>:<d>` coordinate.
///
/// Canonical, not merely parseable: an uppercase pubkey is refused rather
/// than normalized, so two readers cannot disagree about which string names
/// this project.
#[must_use]
pub fn is_canonical_project_coordinate(value: &str) -> bool {
    is_canonical_coordinate(value, "30621", MAX_PROJECT_WORK_PROJECT_REF_BYTES)
}

/// Whether `value` is a canonical `30617:<64 lowercase hex>:<repo id>`
/// coordinate — the full form `planRef.repository` requires.
#[must_use]
pub fn is_canonical_repository_coordinate(value: &str) -> bool {
    if !is_canonical_coordinate(value, "30617", MAX_PROJECT_WORK_REPOSITORY_BYTES) {
        return false;
    }
    value
        .splitn(3, ':')
        .nth(2)
        .is_some_and(crate::project_plan::is_repository_id)
}

fn is_canonical_coordinate(value: &str, kind: &str, max_bytes: usize) -> bool {
    if value.len() > max_bytes {
        return false;
    }
    let mut parts = value.splitn(3, ':');
    let (Some(k), Some(pubkey), Some(d)) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    k == kind && is_hex(pubkey, &[64]) && !d.is_empty() && !d.chars().any(char::is_control)
}

/// Exact keys of an object, with `unknown-key` and `absent-key` told apart.
fn check_keys(object: &Map<String, Value>, expected: &[&str], path: &str) -> Refused<()> {
    for key in object.keys() {
        if !expected.contains(&key.as_str()) {
            return refuse(
                ProjectWorkRefusalCode::UnknownKey,
                format!("{path}.{key}"),
                format!("{path} carries {key:?}, which is not in the closed key set"),
            );
        }
    }
    for key in expected {
        if !object.contains_key(*key) {
            return refuse(
                ProjectWorkRefusalCode::AbsentKey,
                format!("{path}.{key}"),
                format!("{key} must be present and written as null when it has no value"),
            );
        }
    }
    Ok(())
}

fn object<'a>(value: &'a Value, path: &str) -> Refused<&'a Map<String, Value>> {
    value.as_object().map_or_else(
        || {
            refuse(
                ProjectWorkRefusalCode::WrongType,
                path,
                format!("{path} must be an object"),
            )
        },
        Ok,
    )
}

fn string<'a>(object: &'a Map<String, Value>, key: &str, path: &str) -> Refused<&'a str> {
    match object.get(key) {
        Some(Value::String(value)) => Ok(value.as_str()),
        Some(_) => refuse(
            ProjectWorkRefusalCode::WrongType,
            format!("{path}.{key}"),
            format!("{path}.{key} must be a string"),
        ),
        None => refuse(
            ProjectWorkRefusalCode::AbsentKey,
            format!("{path}.{key}"),
            format!("{path}.{key} is required"),
        ),
    }
}

fn nullable_event_id(
    object: &Map<String, Value>,
    key: &str,
    path: &str,
) -> Refused<Option<String>> {
    match object.get(key) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => {
            check_event_id(value, &format!("{path}.{key}"))?;
            Ok(Some(value.clone()))
        }
        Some(_) => refuse(
            ProjectWorkRefusalCode::WrongType,
            format!("{path}.{key}"),
            format!("{path}.{key} is an event id or null"),
        ),
        None => refuse(
            ProjectWorkRefusalCode::AbsentKey,
            format!("{path}.{key}"),
            format!("{key} must be present and written as null"),
        ),
    }
}

fn array<'a>(object: &'a Map<String, Value>, key: &str, path: &str) -> Refused<&'a Vec<Value>> {
    match object.get(key) {
        Some(Value::Array(items)) => Ok(items),
        Some(_) => refuse(
            ProjectWorkRefusalCode::WrongType,
            format!("{path}.{key}"),
            format!("{path}.{key} must be an array"),
        ),
        None => refuse(
            ProjectWorkRefusalCode::AbsentKey,
            format!("{path}.{key}"),
            format!("{path}.{key} is required"),
        ),
    }
}

/// Criterion ids: 1..=64 slugs, each within the slug ceiling, all distinct.
fn criterion_ids(body: &Map<String, Value>, path: &str) -> Refused<Vec<String>> {
    let raw = array(body, "criterionIds", path)?;
    if raw.is_empty() {
        return refuse(
            ProjectWorkRefusalCode::EmptyList,
            format!("{path}.criterionIds"),
            "criterionIds must name at least one criterion",
        );
    }
    if raw.len() > MAX_PROJECT_WORK_CRITERION_IDS {
        return refuse(
            ProjectWorkRefusalCode::TooManyItems,
            format!("{path}.criterionIds"),
            format!("a binding names at most {MAX_PROJECT_WORK_CRITERION_IDS} criteria"),
        );
    }
    let mut ids: Vec<String> = Vec::with_capacity(raw.len());
    for (index, item) in raw.iter().enumerate() {
        let where_ = format!("{path}.criterionIds[{index}]");
        let Value::String(id) = item else {
            return refuse(
                ProjectWorkRefusalCode::WrongType,
                where_,
                "a criterion id is a string",
            );
        };
        if !crate::project_plan::is_plan_slug(id) {
            return refuse(
                ProjectWorkRefusalCode::Slug,
                where_,
                format!("{id:?} is not lowercase slug grammar"),
            );
        }
        if ids.contains(id) {
            return refuse(
                ProjectWorkRefusalCode::Duplicate,
                where_,
                format!("criterionIds repeats {id:?}"),
            );
        }
        ids.push(id.clone());
    }
    Ok(ids)
}

fn plan_ref(body: &Map<String, Value>) -> Refused<ProjectWorkPlanRef> {
    let value = body.get("planRef").ok_or_else(|| {
        ProjectWorkRefusal::new(
            ProjectWorkRefusalCode::AbsentKey,
            "body.planRef",
            "planRef is required",
        )
    })?;
    let map = object(value, "body.planRef")?;
    check_keys(map, &["repository", "commit", "path"], "body.planRef")?;
    let repository = string(map, "repository", "body.planRef")?.to_owned();
    if !is_canonical_repository_coordinate(&repository) {
        return refuse(
            ProjectWorkRefusalCode::Repository,
            "body.planRef.repository",
            "planRef.repository must be the full 30617 coordinate, not a bare repo id",
        );
    }
    let commit = string(map, "commit", "body.planRef")?.to_owned();
    check_commit(&commit, "body.planRef.commit")?;
    let path = string(map, "path", "body.planRef")?.to_owned();
    crate::project_plan::validate_plan_path(&path).map_err(|_| {
        ProjectWorkRefusal::new(
            ProjectWorkRefusalCode::Path,
            "body.planRef.path",
            "planRef.path must start with plans/ and contain no ..",
        )
    })?;
    Ok(ProjectWorkPlanRef {
        repository,
        commit,
        path,
    })
}

fn evidence_refs(body: &Map<String, Value>) -> Refused<Vec<ProjectWorkEvidenceRef>> {
    let raw = array(body, "evidenceRefs", "body")?;
    if raw.is_empty() {
        return refuse(
            ProjectWorkRefusalCode::EmptyList,
            "body.evidenceRefs",
            "an evidence binding with no evidence proves nothing",
        );
    }
    if raw.len() > MAX_PROJECT_WORK_EVIDENCE_REFS {
        return refuse(
            ProjectWorkRefusalCode::TooManyItems,
            "body.evidenceRefs",
            format!("a binding carries at most {MAX_PROJECT_WORK_EVIDENCE_REFS} evidence refs"),
        );
    }
    let mut refs = Vec::with_capacity(raw.len());
    for (index, item) in raw.iter().enumerate() {
        let where_ = format!("body.evidenceRefs[{index}]");
        let map = object(item, &where_)?;
        check_keys(map, &["kind", "eventId"], &where_)?;
        let kind_raw = string(map, "kind", &where_)?;
        let Some(kind) = ProjectWorkEvidenceKind::from_wire(kind_raw) else {
            return refuse(
                ProjectWorkRefusalCode::ClosedEnum,
                format!("{where_}.kind"),
                format!("{kind_raw:?} is not one of report|verdict|action_result|ref_observation"),
            );
        };
        let event_id = string(map, "eventId", &where_)?.to_owned();
        check_event_id(&event_id, &format!("{where_}.eventId"))?;
        refs.push(ProjectWorkEvidenceRef { kind, event_id });
    }
    Ok(refs)
}

/// Decode and validate kind:44249 **content** on its own.
///
/// Everything self-contained in the payload: the closed key sets at every
/// level, the schema, the bounds, and reference syntax. The envelope's tags
/// and their parity with this payload are
/// [`validate_project_work_envelope`]'s question.
///
/// # Errors
///
/// Returns the first [`ProjectWorkRefusal`] found.
pub fn decode_project_work_content(content: &str) -> Refused<ProjectWorkPayload> {
    if content.len() > MAX_PROJECT_WORK_CONTENT_BYTES {
        return refuse(
            ProjectWorkRefusalCode::TooLarge,
            "content",
            format!(
                "work-record content exceeds {MAX_PROJECT_WORK_CONTENT_BYTES} bytes (got {})",
                content.len()
            ),
        );
    }
    let value: Value = serde_json::from_str(content).map_err(|_| {
        ProjectWorkRefusal::new(
            ProjectWorkRefusalCode::Malformed,
            "content",
            "malformed work-record payload",
        )
    })?;
    let payload = object(&value, "content")?;
    // Schema first: a record written to a vocabulary this build does not know
    // is refused as that, never as a pile of unknown keys.
    let schema = string(payload, "schema", "content")?;
    if schema != PROJECT_WORK_SCHEMA {
        return refuse(
            ProjectWorkRefusalCode::Schema,
            "content.schema",
            format!("{schema:?} is not a schema this contract knows"),
        );
    }
    check_keys(
        payload,
        &[
            "schema",
            "sessionRef",
            "genesisRef",
            "projectRef",
            "type",
            "body",
        ],
        "content",
    )?;
    let session_ref = string(payload, "sessionRef", "content")?.to_owned();
    check_uuid(&session_ref, "content.sessionRef")?;
    let genesis_ref = string(payload, "genesisRef", "content")?.to_owned();
    check_event_id(&genesis_ref, "content.genesisRef")?;
    let project_ref = string(payload, "projectRef", "content")?.to_owned();
    if !is_canonical_project_coordinate(&project_ref) {
        return refuse(
            ProjectWorkRefusalCode::ProjectRef,
            "content.projectRef",
            "projectRef is a canonical 30621:<64 lowercase hex>:<d> coordinate",
        );
    }
    let type_raw = string(payload, "type", "content")?;
    let Some(record_type) = ProjectWorkRecordType::from_wire(type_raw) else {
        return refuse(
            ProjectWorkRefusalCode::RecordType,
            "content.type",
            format!("{type_raw:?} is not one of work.declared|work.assignment_bound|work.evidence_bound"),
        );
    };
    let body_value = payload.get("body").ok_or_else(|| {
        ProjectWorkRefusal::new(
            ProjectWorkRefusalCode::AbsentKey,
            "content.body",
            "body is required",
        )
    })?;
    let body_map = object(body_value, "body")?;
    let body = decode_body(record_type, body_map)?;
    Ok(ProjectWorkPayload {
        schema: schema.to_owned(),
        session_ref,
        genesis_ref,
        project_ref,
        record_type,
        body,
    })
}

fn decode_body(
    record_type: ProjectWorkRecordType,
    body: &Map<String, Value>,
) -> Refused<ProjectWorkBody> {
    match record_type {
        ProjectWorkRecordType::Declared => {
            check_keys(
                body,
                &[
                    "workId",
                    "goalRef",
                    "responsibleActor",
                    "planRef",
                    "supersedes",
                ],
                "body",
            )?;
            let work_id = string(body, "workId", "body")?.to_owned();
            check_uuid(&work_id, "body.workId")?;
            let goal_ref = string(body, "goalRef", "body")?.to_owned();
            check_event_id(&goal_ref, "body.goalRef")?;
            let responsible_actor = string(body, "responsibleActor", "body")?.to_owned();
            check_event_id(&responsible_actor, "body.responsibleActor")?;
            let plan = plan_ref(body)?;
            let raw = array(body, "supersedes", "body")?;
            if raw.len() > MAX_PROJECT_WORK_SUPERSEDES {
                return refuse(
                    ProjectWorkRefusalCode::TooManyItems,
                    "body.supersedes",
                    format!(
                        "a declaration supersedes at most {MAX_PROJECT_WORK_SUPERSEDES} others"
                    ),
                );
            }
            let mut supersedes: Vec<String> = Vec::with_capacity(raw.len());
            for (index, item) in raw.iter().enumerate() {
                let where_ = format!("body.supersedes[{index}]");
                let Value::String(id) = item else {
                    return refuse(
                        ProjectWorkRefusalCode::WrongType,
                        where_,
                        "a superseded declaration is named by its event id",
                    );
                };
                check_event_id(id, &where_)?;
                if supersedes.contains(id) {
                    return refuse(
                        ProjectWorkRefusalCode::Duplicate,
                        where_,
                        format!("supersedes repeats {id:?}"),
                    );
                }
                supersedes.push(id.clone());
            }
            Ok(ProjectWorkBody::Declared(ProjectWorkDeclared {
                work_id,
                goal_ref,
                responsible_actor,
                plan_ref: plan,
                supersedes,
            }))
        }
        ProjectWorkRecordType::AssignmentBound => {
            check_keys(
                body,
                &[
                    "declarationRef",
                    "criterionIds",
                    "assignmentRef",
                    "replacesBinding",
                ],
                "body",
            )?;
            let declaration_ref = string(body, "declarationRef", "body")?.to_owned();
            check_event_id(&declaration_ref, "body.declarationRef")?;
            let ids = criterion_ids(body, "body")?;
            let assignment_ref = string(body, "assignmentRef", "body")?.to_owned();
            check_event_id(&assignment_ref, "body.assignmentRef")?;
            let replaces_binding = nullable_event_id(body, "replacesBinding", "body")?;
            Ok(ProjectWorkBody::AssignmentBound(
                ProjectWorkAssignmentBound {
                    declaration_ref,
                    criterion_ids: ids,
                    assignment_ref,
                    replaces_binding,
                },
            ))
        }
        ProjectWorkRecordType::EvidenceBound => {
            check_keys(
                body,
                &[
                    "declarationRef",
                    "criterionIds",
                    "artifactCommit",
                    "evidenceRefs",
                    "completionRef",
                ],
                "body",
            )?;
            let declaration_ref = string(body, "declarationRef", "body")?.to_owned();
            check_event_id(&declaration_ref, "body.declarationRef")?;
            let ids = criterion_ids(body, "body")?;
            let artifact_commit = string(body, "artifactCommit", "body")?.to_owned();
            check_commit(&artifact_commit, "body.artifactCommit")?;
            let refs = evidence_refs(body)?;
            let completion_ref = nullable_event_id(body, "completionRef", "body")?;
            Ok(ProjectWorkBody::EvidenceBound(ProjectWorkEvidenceBound {
                declaration_ref,
                criterion_ids: ids,
                artifact_commit,
                evidence_refs: refs,
                completion_ref,
            }))
        }
    }
}

/// Validate the exact ordered event envelope and return its decoded payload.
///
/// The reader path: the relay's ingest arm, the coverage fold and every
/// surface arrive here with an event that is already signed. Six ordered
/// two-field tags, each value checked in its own right, then the content,
/// then the parity between the two — a tag that disagrees with content is a
/// refusal, not a preference.
///
/// # Errors
///
/// Returns the first [`ProjectWorkRefusal`] found.
pub fn validate_project_work_envelope(event: &ProjectWorkEvent) -> Refused<ProjectWorkPayload> {
    if event.kind != KIND_PROJECT_WORK_RECORD {
        return refuse(
            ProjectWorkRefusalCode::WrongKind,
            "kind",
            format!(
                "a work record is kind {KIND_PROJECT_WORK_RECORD}; this event is kind {}",
                event.kind
            ),
        );
    }
    if event.tags.len() != PROJECT_WORK_TAG_COUNT || event.tags.iter().any(|tag| tag.len() != 2) {
        return refuse(
            ProjectWorkRefusalCode::TagCount,
            "tags",
            format!(
                "a work record carries exactly {PROJECT_WORK_TAG_COUNT} ordered two-field tags; this one carries {}",
                event.tags.len()
            ),
        );
    }
    const NAMES: [&str; PROJECT_WORK_TAG_COUNT] =
        ["h", "d", "a", "pwk-v", "pwk-genesis", "pwk-type"];
    for (index, name) in NAMES.iter().enumerate() {
        if event.tags[index][0] != *name {
            return refuse(
                ProjectWorkRefusalCode::TagCount,
                format!("tags[{index}]"),
                format!("tag {index} must be {name}, not {:?}", event.tags[index][0]),
            );
        }
    }
    check_uuid(&event.tags[0][1], "tags.h")?;
    check_uuid(&event.tags[1][1], "tags.d")?;
    if !is_canonical_project_coordinate(&event.tags[2][1]) {
        return refuse(
            ProjectWorkRefusalCode::ProjectRef,
            "tags.a",
            "the a tag is not a canonical 30621 coordinate",
        );
    }
    if event.tags[3][1] != PROJECT_WORK_SCHEMA {
        return refuse(
            ProjectWorkRefusalCode::Schema,
            "tags.pwk-v",
            format!("{:?} is not a schema this contract knows", event.tags[3][1]),
        );
    }
    check_event_id(&event.tags[4][1], "tags.pwk-genesis")?;
    if ProjectWorkRecordType::from_wire(&event.tags[5][1]).is_none() {
        return refuse(
            ProjectWorkRefusalCode::RecordType,
            "tags.pwk-type",
            format!(
                "{:?} is not a work-record type this contract knows",
                event.tags[5][1]
            ),
        );
    }

    let payload = decode_project_work_content(&event.content)?;
    let parity = [
        ("d", &event.tags[1][1], &payload.session_ref, "sessionRef"),
        ("a", &event.tags[2][1], &payload.project_ref, "projectRef"),
        (
            "pwk-genesis",
            &event.tags[4][1],
            &payload.genesis_ref,
            "genesisRef",
        ),
    ];
    for (tag, tag_value, content_value, key) in parity {
        if tag_value != content_value {
            return refuse(
                ProjectWorkRefusalCode::TagParity,
                format!("tags.{tag}"),
                format!("the {tag} tag names a different value than content.{key}"),
            );
        }
    }
    if event.tags[5][1] != payload.record_type.as_str() {
        return refuse(
            ProjectWorkRefusalCode::TagParity,
            "tags.pwk-type",
            format!(
                "pwk-type says {}, content.type says {}",
                event.tags[5][1],
                payload.record_type.as_str()
            ),
        );
    }
    // A record that names its own event id places itself outside the graph.
    if let ProjectWorkBody::Declared(body) = &payload.body {
        if body.supersedes.contains(&event.id) {
            return refuse(
                ProjectWorkRefusalCode::SelfReference,
                "body.supersedes",
                "the declaration supersedes its own event id",
            );
        }
    }
    if let ProjectWorkBody::AssignmentBound(body) = &payload.body {
        if body.declaration_ref == event.id || body.replaces_binding.as_deref() == Some(&event.id) {
            return refuse(
                ProjectWorkRefusalCode::SelfReference,
                "body",
                "a binding cannot reference its own event id",
            );
        }
    }
    if let ProjectWorkBody::EvidenceBound(body) = &payload.body {
        if body.declaration_ref == event.id
            || body
                .evidence_refs
                .iter()
                .any(|reference| reference.event_id == event.id)
        {
            return refuse(
                ProjectWorkRefusalCode::SelfReference,
                "body",
                "a binding cannot reference its own event id",
            );
        }
    }
    Ok(payload)
}

/// Validate a payload a caller is **about to sign**.
///
/// The same rules the reader applies, reached through the canonical bytes so
/// no author ever signs content the relay and the fold would both reject.
///
/// # Errors
///
/// Returns the first [`ProjectWorkRefusal`] found.
pub fn validate_project_work_payload(payload: &ProjectWorkPayload) -> Refused<()> {
    if payload.record_type != payload.body.record_type() {
        return refuse(
            ProjectWorkRefusalCode::TagParity,
            "content.type",
            "the payload's type and its body disagree",
        );
    }
    let content = payload.canonical_content()?;
    let decoded = decode_project_work_content(&content)?;
    if &decoded != payload {
        return refuse(
            ProjectWorkRefusalCode::Malformed,
            "content",
            "the payload does not round-trip through its canonical form",
        );
    }
    Ok(())
}
