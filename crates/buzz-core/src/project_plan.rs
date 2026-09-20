//! NIP-PW: the `beekeeper-plan/v1` plan file — what success means, in the
//! project's agents repository, at an exact commit.
//!
//! A plan is a UTF-8 Markdown file at `plans/<slug>.md` whose YAML
//! frontmatter is the contract; the body below it is context for people and
//! seats and is never parsed. This module is the parser and nothing else: it
//! is handed **bytes** and returns either a [`Plan`] or a [`PlanRefusal`]
//! carrying a stable string code a CLI or a UI can show. It performs no I/O —
//! reading the blob is the caller's job, and the contract says how
//! (`git show <commit>:<path>`, never the working copy and never the fetched
//! tip).
//!
//! The normative contract is `conformance/project-work/README.md` § (a),
//! restated in `docs/nips/NIP-PW.md`. Every refusal here names the code that
//! file names, and `project_plan_tests.rs` binds each refused fixture to the
//! code the fixture declares, so a rename cannot drift silently.

use serde::{Deserialize, Serialize};

/// Exact schema identifier a v1 plan file must carry.
pub const PROJECT_PLAN_SCHEMA: &str = "beekeeper-plan/v1";
/// Maximum size of a plan **file**, in bytes: the limit is on the file, not
/// on the frontmatter, so a reader refuses before parsing rather than after.
pub const MAX_PLAN_FILE_BYTES: usize = 65_536;
/// Maximum number of active criteria in one plan.
pub const MAX_PLAN_CRITERIA: usize = 64;
/// Maximum number of retired criterion ids in one plan.
pub const MAX_PLAN_RETIRED_CRITERIA: usize = 256;
/// Maximum byte length of one criterion's `accept` text.
pub const MAX_PLAN_ACCEPT_BYTES: usize = 1024;
/// Maximum byte length of any slug: a plan id, a criterion id, an action name
/// or an action step.
pub const MAX_PLAN_SLUG_BYTES: usize = 64;
/// Maximum byte length of a plan `title`.
pub const MAX_PLAN_TITLE_BYTES: usize = 200;
/// Maximum byte length of a `code_repository` id.
pub const MAX_PLAN_CODE_REPOSITORY_BYTES: usize = 64;
/// Maximum byte length of a `delivery_ref`.
pub const MAX_PLAN_DELIVERY_REF_BYTES: usize = 256;
/// Directory every plan path must live under, relative to the agents
/// repository root.
pub const PLAN_PATH_PREFIX: &str = "plans/";
/// Directory whose contents may never be newly adopted.
pub const PLAN_ARCHIVE_PREFIX: &str = "plans/archive/";

/// Whether a plan may be newly adopted.
///
/// `status` governs **new adoption only**: a `superseded` plan refuses a new
/// `adopt` and nothing else. Declarations already adopted keep resolving
/// their own commit and path, because the git commit is the version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlanStatus {
    /// The plan may be adopted.
    InForce,
    /// The plan has been replaced; a new adoption is refused.
    Superseded,
}

impl PlanStatus {
    /// The exact wire token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InForce => "in-force",
            Self::Superseded => "superseded",
        }
    }
}

/// The evidence a criterion requires. Exactly one of three closed forms.
///
/// `proof` is an evidence *requirement*. It is not a command, not a condition
/// language and not a dependency graph: an `action` proof names an entry in
/// the same agents repository's `actions.yml`, which is where commands live
/// under host validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum PlanProof {
    /// A person, or an authorized seat, rules on it.
    Review,
    /// A named action and step in the same agents commit's `actions.yml`.
    Action {
        /// Action name; a slug, resolved at the plan's own commit.
        name: String,
        /// Step within that action; a slug.
        step: String,
    },
    /// The plan's `delivery_ref` is observed at the artifact commit.
    GitRef,
}

/// One obligation: a stable id, the text a person reads to judge it, and the
/// evidence form that answers it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanCriterion {
    /// Slug id, unique among `criteria` and disjoint from `retired_criteria`.
    pub id: String,
    /// What a person reads to judge the obligation. Never copied onto the wire.
    pub accept: String,
    /// The evidence form that answers it.
    pub proof: PlanProof,
}

/// A parsed `beekeeper-plan/v1` file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    /// Always [`PROJECT_PLAN_SCHEMA`].
    pub schema: String,
    /// Plan id; unique within the project.
    pub id: String,
    /// Whether a new adoption is permitted.
    pub status: PlanStatus,
    /// Single-line human title.
    pub title: String,
    /// Repository id of the **code** repository the work delivers into.
    pub code_repository: String,
    /// Full git ref the delivery is judged at, e.g. `refs/heads/main`.
    pub delivery_ref: String,
    /// The active obligations, in presentation order.
    pub criteria: Vec<PlanCriterion>,
    /// Ids retired from this plan; never recycled.
    pub retired_criteria: Vec<String>,
    /// Size of the file this plan was parsed from, in bytes.
    pub bytes: usize,
}

impl Plan {
    /// The branch-or-tag portion of `delivery_ref`, if it names one.
    ///
    /// Returns the whole ref unchanged: the fold matches the ref **name** as
    /// a kind:30618 tag, and 30618 carries `refs/heads/main` verbatim.
    #[must_use]
    pub fn delivery_ref_tag(&self) -> &str {
        &self.delivery_ref
    }

    /// Whether `id` names an active criterion of this plan.
    #[must_use]
    pub fn has_criterion(&self, id: &str) -> bool {
        self.criteria.iter().any(|criterion| criterion.id == id)
    }
}

/// Why a plan file was refused, as a stable code a CLI and a UI can show.
///
/// The strings are the contract's, not this module's: they appear in
/// `conformance/project-work/README.md` and in the `# REFUSED:` comment of
/// every invalid plan fixture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanRefusalCode {
    /// The file is larger than [`MAX_PLAN_FILE_BYTES`].
    PlanTooLarge,
    /// The file is not UTF-8, or has no `---` frontmatter block.
    MissingFrontmatter,
    /// The frontmatter is not well-formed YAML, or is not a mapping.
    MalformedFrontmatter,
    /// `schema` is absent or is not [`PROJECT_PLAN_SCHEMA`].
    UnknownSchema,
    /// A frontmatter key outside the closed set.
    UnknownFrontmatterKey,
    /// A required frontmatter key is absent.
    MissingFrontmatterKey,
    /// A key inside a criterion entry outside the closed set.
    UnknownCriterionKey,
    /// A key inside a `proof` outside that form's closed set.
    UnknownProofKey,
    /// A `proof.kind` outside `review` | `action` | `git-ref`.
    UnknownProofKind,
    /// `status` is neither `in-force` nor `superseded`.
    UnknownStatus,
    /// A value has the wrong YAML type.
    WrongType,
    /// More than [`MAX_PLAN_CRITERIA`] active criteria.
    TooManyCriteria,
    /// More than [`MAX_PLAN_RETIRED_CRITERIA`] retired ids.
    TooManyRetired,
    /// No criteria at all: a plan with nothing to judge is not a contract.
    NoCriteria,
    /// An `accept` longer than [`MAX_PLAN_ACCEPT_BYTES`].
    AcceptTooLong,
    /// An `accept` that is empty or whitespace only.
    EmptyAccept,
    /// A slug longer than [`MAX_PLAN_SLUG_BYTES`].
    SlugTooLong,
    /// The plan `id` is not slug grammar.
    PlanIdNotASlug,
    /// A criterion `id` is not slug grammar.
    CriterionIdNotASlug,
    /// A retired id is not slug grammar.
    RetiredIdNotASlug,
    /// A criterion id appears twice under `criteria`.
    DuplicateCriterionId,
    /// A retired id appears twice under `retired_criteria`.
    DuplicateRetiredId,
    /// An id is active and retired at once — the recycling this forbids.
    RecycledRetiredId,
    /// A `proof.name` is not a slug: a path, a `..` or a shell string.
    ActionNameNotASlug,
    /// A `proof.step` is not a slug.
    ActionStepNotASlug,
    /// `title` is empty, multi-line, or over [`MAX_PLAN_TITLE_BYTES`].
    InvalidTitle,
    /// `code_repository` is not a repository id.
    InvalidCodeRepository,
    /// `delivery_ref` is not a full git ref, or carries a glob.
    InvalidDeliveryRef,
    /// A plan path that does not start `plans/`, or escapes it.
    PlanPathEscapes,
    /// A plan path that is absolute, empty, or not a `.md` file.
    PlanPathNotRelative,
    /// A plan path that resolved through a symlink.
    ///
    /// The parser cannot observe this — it never touches a filesystem — so
    /// the code exists for the reader that does, and keeps the refusal
    /// vocabulary in one place.
    PlanPathSymlinked,
    /// A new adoption of a `superseded` plan, or of a `plans/archive/` path.
    NotAdoptable,
}

impl PlanRefusalCode {
    /// The stable string form, as the contract writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PlanTooLarge => "plan-too-large",
            Self::MissingFrontmatter => "missing-frontmatter",
            Self::MalformedFrontmatter => "malformed-frontmatter",
            Self::UnknownSchema => "unknown-schema",
            Self::UnknownFrontmatterKey => "unknown-frontmatter-key",
            Self::MissingFrontmatterKey => "missing-frontmatter-key",
            Self::UnknownCriterionKey => "unknown-criterion-key",
            Self::UnknownProofKey => "unknown-proof-key",
            Self::UnknownProofKind => "unknown-proof-kind",
            Self::UnknownStatus => "unknown-status",
            Self::WrongType => "wrong-type",
            Self::TooManyCriteria => "too-many-criteria",
            Self::TooManyRetired => "too-many-retired",
            Self::NoCriteria => "no-criteria",
            Self::AcceptTooLong => "accept-too-long",
            Self::EmptyAccept => "empty-accept",
            Self::SlugTooLong => "slug-too-long",
            Self::PlanIdNotASlug => "plan-id-not-a-slug",
            Self::CriterionIdNotASlug => "criterion-id-not-a-slug",
            Self::RetiredIdNotASlug => "retired-id-not-a-slug",
            Self::DuplicateCriterionId => "duplicate-criterion-id",
            Self::DuplicateRetiredId => "duplicate-retired-id",
            Self::RecycledRetiredId => "recycled-retired-id",
            Self::ActionNameNotASlug => "action-name-not-a-slug",
            Self::ActionStepNotASlug => "action-step-not-a-slug",
            Self::InvalidTitle => "invalid-title",
            Self::InvalidCodeRepository => "invalid-code-repository",
            Self::InvalidDeliveryRef => "invalid-delivery-ref",
            Self::PlanPathEscapes => "plan-path-escapes",
            Self::PlanPathNotRelative => "plan-path-not-relative",
            Self::PlanPathSymlinked => "plan-path-symlinked",
            Self::NotAdoptable => "not-adoptable",
        }
    }
}

impl std::fmt::Display for PlanRefusalCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One refusal: the code, where in the document it was found, and why.
///
/// `path` is the contract's own dotted/indexed notation
/// (`criteria[2].id`), so the CLI's `{"code", "path", "message"}` error
/// object is this struct renamed and nothing more.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanRefusal {
    /// The stable code.
    pub code: PlanRefusalCode,
    /// Where the defect is, e.g. `criteria[2].id` or `frontmatter`.
    pub path: String,
    /// One sentence a person can act on.
    pub message: String,
}

impl PlanRefusal {
    fn new(code: PlanRefusalCode, path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code,
            path: path.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for PlanRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {} ({})", self.code, self.message, self.path)
    }
}

impl Serialize for PlanRefusalCode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for PlanRefusalCode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        plan_refusal_code_from_str(&raw)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown plan refusal code {raw}")))
    }
}

/// Resolve a stable refusal code string back to its variant.
///
/// The inverse of [`PlanRefusalCode::as_str`], and the reason a fixture can
/// declare `# REFUSED: <code>` and a test can bind to it without a second,
/// drifting table.
#[must_use]
pub fn plan_refusal_code_from_str(raw: &str) -> Option<PlanRefusalCode> {
    use PlanRefusalCode as C;
    const ALL: [PlanRefusalCode; 32] = [
        C::PlanTooLarge,
        C::MissingFrontmatter,
        C::MalformedFrontmatter,
        C::UnknownSchema,
        C::UnknownFrontmatterKey,
        C::MissingFrontmatterKey,
        C::UnknownCriterionKey,
        C::UnknownProofKey,
        C::UnknownProofKind,
        C::UnknownStatus,
        C::WrongType,
        C::TooManyCriteria,
        C::TooManyRetired,
        C::NoCriteria,
        C::AcceptTooLong,
        C::EmptyAccept,
        C::SlugTooLong,
        C::PlanIdNotASlug,
        C::CriterionIdNotASlug,
        C::RetiredIdNotASlug,
        C::DuplicateCriterionId,
        C::DuplicateRetiredId,
        C::RecycledRetiredId,
        C::ActionNameNotASlug,
        C::ActionStepNotASlug,
        C::InvalidTitle,
        C::InvalidCodeRepository,
        C::InvalidDeliveryRef,
        C::PlanPathEscapes,
        C::PlanPathNotRelative,
        C::PlanPathSymlinked,
        C::NotAdoptable,
    ];
    ALL.into_iter().find(|code| code.as_str() == raw)
}

/// Whether `value` is the contract's slug grammar.
///
/// `[a-z0-9] ( [a-z0-9-]* [a-z0-9] )?`, 1..=64 bytes, ASCII only. Lowercase,
/// digits and internal hyphens: no leading or trailing hyphen, no underscore,
/// no dot, no slash, no uppercase.
#[must_use]
pub fn is_plan_slug(value: &str) -> bool {
    if value.is_empty() || value.len() > MAX_PLAN_SLUG_BYTES {
        return false;
    }
    let bytes = value.as_bytes();
    let ok = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    if !ok(bytes[0]) || !ok(bytes[bytes.len() - 1]) {
        return false;
    }
    bytes.iter().all(|&b| ok(b) || b == b'-')
}

/// Whether `value` is a repository id: `[a-z0-9][a-z0-9-]*`, 1..=64 bytes.
#[must_use]
pub fn is_repository_id(value: &str) -> bool {
    if value.is_empty() || value.len() > MAX_PLAN_CODE_REPOSITORY_BYTES {
        return false;
    }
    let bytes = value.as_bytes();
    (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Validate a plan **path** without touching a filesystem.
///
/// Relative, under `plans/`, no `..`, no leading `/`, a `.md` file, and
/// within [`MAX_PLAN_PATH_BYTES`]. Whether the path resolved through a
/// symlink is the reader's question — see [`PlanRefusalCode::PlanPathSymlinked`].
pub fn validate_plan_path(path: &str) -> Result<(), PlanRefusal> {
    if path.is_empty() || path.len() > MAX_PLAN_PATH_BYTES {
        return Err(PlanRefusal::new(
            PlanRefusalCode::PlanPathNotRelative,
            "path",
            format!("a plan path is 1..={MAX_PLAN_PATH_BYTES} bytes"),
        ));
    }
    if path.starts_with('/') || path.contains('\\') || path.contains("//") {
        return Err(PlanRefusal::new(
            PlanRefusalCode::PlanPathNotRelative,
            "path",
            "a plan path is relative, with forward slashes and no empty segment",
        ));
    }
    if !path.starts_with(PLAN_PATH_PREFIX) || path.split('/').any(|segment| segment == "..") {
        return Err(PlanRefusal::new(
            PlanRefusalCode::PlanPathEscapes,
            "path",
            format!("a plan path starts with {PLAN_PATH_PREFIX} and contains no .."),
        ));
    }
    if !path.ends_with(".md") {
        return Err(PlanRefusal::new(
            PlanRefusalCode::PlanPathNotRelative,
            "path",
            "a plan file is Markdown: the path ends in .md",
        ));
    }
    Ok(())
}

/// Maximum byte length of a plan path, matching `planRef.path` on the wire.
pub const MAX_PLAN_PATH_BYTES: usize = 256;

/// Whether this plan and path may be **newly adopted**.
///
/// The one thing `status` governs. Already-adopted declarations are not
/// affected by either answer: editing, renaming or archiving a file cancels
/// nothing, because the commit is the version.
pub fn check_plan_adoptable(plan: &Plan, path: &str) -> Result<(), PlanRefusal> {
    if plan.status == PlanStatus::Superseded {
        return Err(PlanRefusal::new(
            PlanRefusalCode::NotAdoptable,
            "status",
            "a superseded plan cannot be newly adopted; adopt its successor",
        ));
    }
    if path.starts_with(PLAN_ARCHIVE_PREFIX) {
        return Err(PlanRefusal::new(
            PlanRefusalCode::NotAdoptable,
            "path",
            format!("a plan under {PLAN_ARCHIVE_PREFIX} cannot be newly adopted"),
        ));
    }
    Ok(())
}

/// The closed frontmatter key set, in contract order.
const FRONTMATTER_KEYS: [&str; 8] = [
    "schema",
    "id",
    "status",
    "title",
    "code_repository",
    "delivery_ref",
    "criteria",
    "retired_criteria",
];

/// Parse a `beekeeper-plan/v1` file from its exact bytes.
///
/// The caller supplies what `git show <commit>:<path>` produced. Nothing here
/// reads a file, a working copy or a network, and nothing here consults a
/// clock: the same bytes always produce the same answer.
///
/// Every rule in `conformance/project-work/README.md` § (a) is in force, and
/// an unknown key is a refusal rather than an ignored key — a key a reader
/// drops is a promise somebody believed they had made.
///
/// # Errors
///
/// Returns the first [`PlanRefusal`] found, with the stable code the contract
/// names for that defect.
pub fn parse_plan(bytes: &[u8]) -> Result<Plan, PlanRefusal> {
    if bytes.len() > MAX_PLAN_FILE_BYTES {
        return Err(PlanRefusal::new(
            PlanRefusalCode::PlanTooLarge,
            "file",
            format!(
                "a plan file is at most {MAX_PLAN_FILE_BYTES} bytes; this one is {}",
                bytes.len()
            ),
        ));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| {
        PlanRefusal::new(
            PlanRefusalCode::MissingFrontmatter,
            "file",
            "a plan file is UTF-8",
        )
    })?;
    let frontmatter = split_frontmatter(text)?;
    let document: serde_yaml::Value = serde_yaml::from_str(frontmatter).map_err(|error| {
        PlanRefusal::new(
            PlanRefusalCode::MalformedFrontmatter,
            "frontmatter",
            format!("the YAML frontmatter does not parse: {error}"),
        )
    })?;
    let mapping = document.as_mapping().ok_or_else(|| {
        PlanRefusal::new(
            PlanRefusalCode::MalformedFrontmatter,
            "frontmatter",
            "the YAML frontmatter must be a mapping of the contract's keys",
        )
    })?;

    // Schema first, so a file written to a vocabulary this build does not
    // know is refused as that, and never as a pile of unknown keys.
    let schema = required_str(mapping, "schema")?;
    if schema != PROJECT_PLAN_SCHEMA {
        return Err(PlanRefusal::new(
            PlanRefusalCode::UnknownSchema,
            "schema",
            format!("schema must be exactly {PROJECT_PLAN_SCHEMA}, not {schema:?}"),
        ));
    }
    for key in mapping.keys() {
        let name = key.as_str().ok_or_else(|| {
            PlanRefusal::new(
                PlanRefusalCode::UnknownFrontmatterKey,
                "frontmatter",
                "every frontmatter key is a string",
            )
        })?;
        if !FRONTMATTER_KEYS.contains(&name) {
            return Err(PlanRefusal::new(
                PlanRefusalCode::UnknownFrontmatterKey,
                format!("frontmatter.{name}"),
                format!("{name:?} is not a key in {PROJECT_PLAN_SCHEMA}"),
            ));
        }
    }
    for key in FRONTMATTER_KEYS {
        if !mapping.contains_key(serde_yaml::Value::from(key)) {
            return Err(PlanRefusal::new(
                PlanRefusalCode::MissingFrontmatterKey,
                format!("frontmatter.{key}"),
                format!("{key} is required; retired_criteria is written even when empty"),
            ));
        }
    }

    let id = required_str(mapping, "id")?.to_owned();
    check_slug(
        &id,
        "id",
        PlanRefusalCode::PlanIdNotASlug,
        "a plan id is a slug",
    )?;

    let status_raw = required_str(mapping, "status")?;
    let status = match status_raw {
        "in-force" => PlanStatus::InForce,
        "superseded" => PlanStatus::Superseded,
        other => {
            return Err(PlanRefusal::new(
                PlanRefusalCode::UnknownStatus,
                "status",
                format!("status is in-force or superseded, not {other:?}"),
            ))
        }
    };

    let title = required_str(mapping, "title")?.to_owned();
    if title.trim().is_empty() || title.len() > MAX_PLAN_TITLE_BYTES || title.contains('\n') {
        return Err(PlanRefusal::new(
            PlanRefusalCode::InvalidTitle,
            "title",
            format!("title is one non-empty line of at most {MAX_PLAN_TITLE_BYTES} bytes"),
        ));
    }

    let code_repository = required_str(mapping, "code_repository")?.to_owned();
    if !is_repository_id(&code_repository) {
        return Err(PlanRefusal::new(
            PlanRefusalCode::InvalidCodeRepository,
            "code_repository",
            format!("code_repository is a repository id, not {code_repository:?}"),
        ));
    }

    let delivery_ref = required_str(mapping, "delivery_ref")?.to_owned();
    check_delivery_ref(&delivery_ref)?;

    let criteria = parse_criteria(mapping)?;
    let retired_criteria = parse_retired(mapping, &criteria)?;

    Ok(Plan {
        schema: schema.to_owned(),
        id,
        status,
        title,
        code_repository,
        delivery_ref,
        criteria,
        retired_criteria,
        bytes: bytes.len(),
    })
}

/// Return the YAML frontmatter between the opening and closing `---` fences.
fn split_frontmatter(text: &str) -> Result<&str, PlanRefusal> {
    let missing = || {
        PlanRefusal::new(
            PlanRefusalCode::MissingFrontmatter,
            "file",
            "a plan file opens with a --- YAML frontmatter block",
        )
    };
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
        .ok_or_else(missing)?;
    let end = rest
        .match_indices("\n---")
        .find(|(index, _)| {
            let after = &rest[index + 4..];
            after.is_empty() || after.starts_with('\n') || after.starts_with('\r')
        })
        .map(|(index, _)| index)
        .ok_or_else(missing)?;
    Ok(&rest[..end])
}

fn required_str<'a>(mapping: &'a serde_yaml::Mapping, key: &str) -> Result<&'a str, PlanRefusal> {
    let value = mapping.get(serde_yaml::Value::from(key)).ok_or_else(|| {
        PlanRefusal::new(
            PlanRefusalCode::MissingFrontmatterKey,
            format!("frontmatter.{key}"),
            format!("{key} is required"),
        )
    })?;
    value.as_str().ok_or_else(|| {
        PlanRefusal::new(
            PlanRefusalCode::WrongType,
            format!("frontmatter.{key}"),
            format!("{key} is a string"),
        )
    })
}

fn check_slug(
    value: &str,
    path: &str,
    code: PlanRefusalCode,
    what: &str,
) -> Result<(), PlanRefusal> {
    if value.len() > MAX_PLAN_SLUG_BYTES {
        return Err(PlanRefusal::new(
            PlanRefusalCode::SlugTooLong,
            path.to_owned(),
            format!("a slug is at most {MAX_PLAN_SLUG_BYTES} bytes"),
        ));
    }
    if !is_plan_slug(value) {
        return Err(PlanRefusal::new(
            code,
            path.to_owned(),
            format!("{what}: lowercase letters, digits and internal hyphens, not {value:?}"),
        ));
    }
    Ok(())
}

fn check_delivery_ref(value: &str) -> Result<(), PlanRefusal> {
    let refused = |why: &str| {
        Err(PlanRefusal::new(
            PlanRefusalCode::InvalidDeliveryRef,
            "delivery_ref",
            why.to_owned(),
        ))
    };
    if value.is_empty() || value.len() > MAX_PLAN_DELIVERY_REF_BYTES {
        return refused("delivery_ref is 1..=256 bytes");
    }
    if !value.starts_with("refs/") {
        return refused("delivery_ref is a full git ref, e.g. refs/heads/main");
    }
    if value.chars().any(|c| {
        c.is_whitespace() || c.is_control() || matches!(c, '*' | '?' | '[' | '^' | '~' | ':' | '\\')
    }) || value.contains("..")
        || value.ends_with('/')
        || value.contains("//")
    {
        return refused("delivery_ref carries no glob, no .. and no empty segment");
    }
    Ok(())
}

fn parse_criteria(mapping: &serde_yaml::Mapping) -> Result<Vec<PlanCriterion>, PlanRefusal> {
    let raw = mapping
        .get(serde_yaml::Value::from("criteria"))
        .and_then(serde_yaml::Value::as_sequence)
        .ok_or_else(|| {
            PlanRefusal::new(
                PlanRefusalCode::WrongType,
                "criteria",
                "criteria is a list of entries",
            )
        })?;
    if raw.is_empty() {
        return Err(PlanRefusal::new(
            PlanRefusalCode::NoCriteria,
            "criteria",
            "a plan states at least one criterion; there is nothing to judge otherwise",
        ));
    }
    if raw.len() > MAX_PLAN_CRITERIA {
        return Err(PlanRefusal::new(
            PlanRefusalCode::TooManyCriteria,
            "criteria",
            format!("a plan carries at most {MAX_PLAN_CRITERIA} criteria"),
        ));
    }
    let mut criteria = Vec::with_capacity(raw.len());
    let mut seen: Vec<String> = Vec::with_capacity(raw.len());
    for (index, entry) in raw.iter().enumerate() {
        let entry = entry.as_mapping().ok_or_else(|| {
            PlanRefusal::new(
                PlanRefusalCode::WrongType,
                format!("criteria[{index}]"),
                "a criterion is a mapping of id, accept and proof",
            )
        })?;
        for key in entry.keys() {
            let name = key.as_str().unwrap_or_default();
            if !matches!(name, "id" | "accept" | "proof") {
                return Err(PlanRefusal::new(
                    PlanRefusalCode::UnknownCriterionKey,
                    format!("criteria[{index}].{name}"),
                    format!("a criterion has exactly id, accept and proof; {name:?} is not one"),
                ));
            }
        }
        let id = required_str(entry, "id")
            .map_err(|refusal| reframe(refusal, &format!("criteria[{index}].id")))?
            .to_owned();
        check_slug(
            &id,
            &format!("criteria[{index}].id"),
            PlanRefusalCode::CriterionIdNotASlug,
            "a criterion id is a slug",
        )?;
        if seen.contains(&id) {
            return Err(PlanRefusal::new(
                PlanRefusalCode::DuplicateCriterionId,
                format!("criteria[{index}].id"),
                format!("criterion id {id:?} appears twice"),
            ));
        }
        seen.push(id.clone());

        let accept = required_str(entry, "accept")
            .map_err(|refusal| reframe(refusal, &format!("criteria[{index}].accept")))?
            .to_owned();
        if accept.len() > MAX_PLAN_ACCEPT_BYTES {
            return Err(PlanRefusal::new(
                PlanRefusalCode::AcceptTooLong,
                format!("criteria[{index}].accept"),
                format!("accept is at most {MAX_PLAN_ACCEPT_BYTES} bytes"),
            ));
        }
        if accept.trim().is_empty() {
            return Err(PlanRefusal::new(
                PlanRefusalCode::EmptyAccept,
                format!("criteria[{index}].accept"),
                format!("{id:?} has no acceptance text, so it cannot be judged"),
            ));
        }
        let proof = parse_proof(entry, index)?;
        criteria.push(PlanCriterion { id, accept, proof });
    }
    Ok(criteria)
}

fn parse_proof(entry: &serde_yaml::Mapping, index: usize) -> Result<PlanProof, PlanRefusal> {
    let proof = entry
        .get(serde_yaml::Value::from("proof"))
        .and_then(serde_yaml::Value::as_mapping)
        .ok_or_else(|| {
            PlanRefusal::new(
                PlanRefusalCode::WrongType,
                format!("criteria[{index}].proof"),
                "proof is a mapping with a kind",
            )
        })?;
    let kind = required_str(proof, "kind")
        .map_err(|refusal| reframe(refusal, &format!("criteria[{index}].proof.kind")))?;
    let expected: &[&str] = match kind {
        "review" | "git-ref" => &["kind"],
        "action" => &["kind", "name", "step"],
        other => {
            return Err(PlanRefusal::new(
                PlanRefusalCode::UnknownProofKind,
                format!("criteria[{index}].proof.kind"),
                format!("proof kind is review, action or git-ref, not {other:?}"),
            ))
        }
    };
    for key in proof.keys() {
        let name = key.as_str().unwrap_or_default();
        if !expected.contains(&name) {
            return Err(PlanRefusal::new(
                PlanRefusalCode::UnknownProofKey,
                format!("criteria[{index}].proof.{name}"),
                format!("a {kind} proof has exactly {expected:?}; {name:?} is not one"),
            ));
        }
    }
    for key in expected {
        if !proof.contains_key(serde_yaml::Value::from(*key)) {
            return Err(PlanRefusal::new(
                PlanRefusalCode::UnknownProofKey,
                format!("criteria[{index}].proof.{key}"),
                format!("a {kind} proof requires {key}"),
            ));
        }
    }
    match kind {
        "review" => Ok(PlanProof::Review),
        "git-ref" => Ok(PlanProof::GitRef),
        _ => {
            let name = required_str(proof, "name")
                .map_err(|refusal| reframe(refusal, &format!("criteria[{index}].proof.name")))?
                .to_owned();
            check_slug(
                &name,
                &format!("criteria[{index}].proof.name"),
                PlanRefusalCode::ActionNameNotASlug,
                "an action proof names an actions.yml entry, never a path or a command",
            )?;
            let step = required_str(proof, "step")
                .map_err(|refusal| reframe(refusal, &format!("criteria[{index}].proof.step")))?
                .to_owned();
            check_slug(
                &step,
                &format!("criteria[{index}].proof.step"),
                PlanRefusalCode::ActionStepNotASlug,
                "an action step is a slug",
            )?;
            Ok(PlanProof::Action { name, step })
        }
    }
}

fn parse_retired(
    mapping: &serde_yaml::Mapping,
    criteria: &[PlanCriterion],
) -> Result<Vec<String>, PlanRefusal> {
    let raw = mapping
        .get(serde_yaml::Value::from("retired_criteria"))
        .and_then(serde_yaml::Value::as_sequence)
        .ok_or_else(|| {
            PlanRefusal::new(
                PlanRefusalCode::WrongType,
                "retired_criteria",
                "retired_criteria is a list, written as [] when empty",
            )
        })?;
    if raw.len() > MAX_PLAN_RETIRED_CRITERIA {
        return Err(PlanRefusal::new(
            PlanRefusalCode::TooManyRetired,
            "retired_criteria",
            format!("a plan retires at most {MAX_PLAN_RETIRED_CRITERIA} ids"),
        ));
    }
    let mut retired: Vec<String> = Vec::with_capacity(raw.len());
    for (index, entry) in raw.iter().enumerate() {
        let path = format!("retired_criteria[{index}]");
        let id = entry
            .as_str()
            .ok_or_else(|| {
                PlanRefusal::new(
                    PlanRefusalCode::WrongType,
                    path.clone(),
                    "a retired criterion id is a string",
                )
            })?
            .to_owned();
        check_slug(
            &id,
            &path,
            PlanRefusalCode::RetiredIdNotASlug,
            "a retired criterion id is a slug",
        )?;
        if retired.contains(&id) {
            return Err(PlanRefusal::new(
                PlanRefusalCode::DuplicateRetiredId,
                path,
                format!("retired id {id:?} appears twice"),
            ));
        }
        if criteria.iter().any(|criterion| criterion.id == id) {
            return Err(PlanRefusal::new(
                PlanRefusalCode::RecycledRetiredId,
                path,
                format!("{id:?} is retired and active at once; a retired id is never recycled"),
            ));
        }
        retired.push(id);
    }
    Ok(retired)
}

/// Re-point a refusal at the exact document path, keeping its code.
fn reframe(refusal: PlanRefusal, path: &str) -> PlanRefusal {
    PlanRefusal {
        code: refusal.code,
        path: path.to_owned(),
        message: refusal.message,
    }
}

#[cfg(test)]
#[path = "project_plan_tests.rs"]
mod tests;
