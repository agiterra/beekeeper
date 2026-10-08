use super::*;
use crate::commands::preview::{execute_with_client, exit_code_for, wants_relay, PreviewCmd};

use beekeeper_core::coding_session_command::CodingSessionTarget;
use beekeeper_core::session_preview::{PreviewStatus, PreviewStream, SessionPreviewAnnounce};
use clap::Parser;
use nostr::{Keys, Timestamp};

const CHANNEL: &str = "6f1c1c0e-6a8e-4c38-9d0f-1a2b3c4d5e6f";
const REF_A: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const REF_B: &str = "0a7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a11";

#[derive(Debug, Parser)]
#[command(name = "preview")]
struct TestCli {
    #[command(subcommand)]
    cmd: PreviewCmd,
}

fn parse(args: &[&str]) -> Result<PreviewCmd, clap::Error> {
    let mut argv = vec!["preview"];
    argv.extend_from_slice(args);
    TestCli::try_parse_from(argv).map(|cli| cli.cmd)
}

fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&[0, 0, 0, 0]); // CRC: copied, never checked here
    out
}

fn png(extra: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
    let mut ihdr = 1280u32.to_be_bytes().to_vec();
    ihdr.extend_from_slice(&800u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(chunk(b"IHDR", &ihdr));
    for (kind, data) in extra {
        out.extend(chunk(kind, data));
    }
    out.extend(chunk(b"IDAT", b"pixels"));
    out.extend(chunk(b"IEND", b""));
    out
}

fn has_chunk(bytes: &[u8], kind: &[u8; 4]) -> bool {
    bytes.windows(4).any(|window| window == kind)
}

#[test]
fn share_and_alt_parse_and_need_each_other() {
    let cmd = parse(&["snapshot", "--share", "--alt", "after save"]).expect("parses");
    assert!(wants_relay(&cmd));
    match cmd {
        PreviewCmd::Snapshot { share, alt, .. } => {
            assert!(share);
            assert_eq!(alt.as_deref(), Some("after save"));
        }
        other => panic!("{other:?}"),
    }
    assert!(!wants_relay(&parse(&["snapshot"]).expect("parses")));
    assert!(
        parse(&["snapshot", "--alt", "x"]).is_err(),
        "--alt needs --share"
    );
    assert!(
        parse(&["snapshot", "--share", "--no-image"]).is_err(),
        "nothing to share"
    );
}

#[test]
fn share_failure_codes_have_their_exit_codes() {
    assert_eq!(exit_code_for("preview_share_no_relay"), 2);
    assert_eq!(exit_code_for("preview_share_no_identity"), 3);
    assert_eq!(exit_code_for("preview_share_not_announced"), 4);
    assert_eq!(exit_code_for("preview_share_page_refused"), 4);
    assert_eq!(exit_code_for("preview_share_upload_failed"), 4);
}

#[tokio::test]
async fn share_without_a_relay_refuses_before_asking_the_broker() {
    let dir = tempfile::tempdir().expect("tempdir");
    let context = PreviewContext {
        socket: dir.path().join("no-broker.sock"),
        grant: None,
        caller: None,
        cwd: dir.path().to_path_buf(),
        now_ms: 1,
    };
    let cmd = parse(&["snapshot", "--share"]).expect("parses");
    let failure = execute_with_client(&cmd, &context, None)
        .await
        .expect_err("no relay");
    assert_eq!(failure.code, "preview_share_no_relay");
    assert_eq!(failure.exit_code(), 2);
    let plain = parse(&["snapshot"]).expect("parses");
    let failure = execute_with_client(&plain, &context, None)
        .await
        .expect_err("no broker");
    assert_eq!(
        failure.code, "preview_no_browser",
        "without --share nothing changes"
    );
}

#[test]
fn metadata_chunks_are_stripped_and_rendering_chunks_kept() {
    let source = png(&[
        (b"tEXt", b"Comment\0/Users/brian/secret"),
        (b"iCCP", b"profile"),
        (b"pHYs", b"123456789"),
        (b"sRGB", b"\0"),
    ]);
    let stripped = strip_png_metadata(&source).expect("strip");
    for gone in [b"tEXt", b"iCCP", b"pHYs"] {
        assert!(!has_chunk(&stripped, gone));
    }
    for kept in [b"IHDR", b"sRGB", b"IDAT", b"IEND"] {
        assert!(has_chunk(&stripped, kept));
    }
    assert_eq!(png_dimensions(&stripped), Some((1280, 800)));
    assert_eq!(strip_png_metadata(&png(&[])).expect("clean"), png(&[]));

    assert!(strip_png_metadata(b"GIF89a").is_err());
    let truncated = &source[..source.len() - 6];
    assert!(strip_png_metadata(truncated).is_err());
}

#[test]
fn the_commit_is_head_plus_dirtiness() {
    let sha = "1a2b3c4d".repeat(5);
    let clean = commit_from_git(&format!("{sha}\n"), "").expect("commit");
    assert_eq!((clean.sha.as_str(), clean.dirty), (sha.as_str(), false));
    assert!(
        commit_from_git(&sha, " M src/lib.rs\n")
            .expect("commit")
            .dirty
    );
    assert_eq!(commit_from_git("fatal: not a git repository", ""), None);
}

#[test]
fn free_text_is_one_redacted_bounded_line() {
    let text = clean_text("Saved\nsee /Users/brian/project/out.txt", 200);
    assert!(!text.contains('\n'));
    assert!(!text.contains("/Users/"), "{text}");
    assert!(clean_text(&"é".repeat(300), 200).len() <= 200);
}

fn target_key(generation: u64) -> String {
    coding_session_target_key(&CodingSessionTarget {
        driver: "claude".into(),
        instance_id: "inst-1".into(),
        session_id: "S".into(),
        generation,
    })
}

fn announce(
    session_ref: &str,
    status: PreviewStatus,
    target: Option<String>,
) -> SessionPreviewAnnounce {
    SessionPreviewAnnounce {
        channel_id: Uuid::parse_str(CHANNEL).expect("uuid"),
        session_ref: session_ref.into(),
        status,
        target_key: target,
        provider: None,
        // A close carries no page or title (NIP-SP).
        page: (status == PreviewStatus::Open).then(|| "local:/".into()),
        title: (status == PreviewStatus::Open).then(String::new),
        viewport_width: 1280,
        viewport_height: 800,
        stream: PreviewStream::Frames,
    }
}

fn signed(a: &SessionPreviewAnnounce, keys: &Keys, at: u64) -> Event {
    beekeeper_sdk::surface::build_session_preview_announce(a)
        .expect("announce")
        .custom_created_at(Timestamp::from(at))
        .sign_with_keys(keys)
        .expect("sign")
}

#[test]
fn the_shared_preview_is_this_generations_or_the_only_one() {
    let channel = Uuid::parse_str(CHANNEL).expect("uuid");
    let desktop = Keys::generate();
    let mine = target_key(2);
    let a = signed(
        &announce(REF_A, PreviewStatus::Open, Some(mine.clone())),
        &desktop,
        100,
    );
    let b = signed(&announce(REF_B, PreviewStatus::Open, None), &desktop, 110);

    let picked = pick_session_ref(&[a.clone(), b.clone()], channel, Some(&mine)).expect("mine");
    assert_eq!(picked, (REF_A.to_owned(), desktop.public_key()));
    assert_eq!(
        pick_session_ref(std::slice::from_ref(&b), channel, Some(&mine))
            .expect("only")
            .0,
        REF_B
    );
    assert!(matches!(
        pick_session_ref(&[a.clone(), b.clone()], channel, Some(&target_key(3))),
        Err(PickError::Ambiguous(refs)) if refs.len() == 2
    ));
    let closed = signed(
        &announce(REF_A, PreviewStatus::Closed, Some(mine.clone())),
        &desktop,
        200,
    );
    assert_eq!(
        pick_session_ref(&[a, closed], channel, Some(&mine)),
        Err(PickError::None)
    );
    assert_eq!(pick_session_ref(&[], channel, None), Err(PickError::None));
    assert_eq!(
        NOT_ANNOUNCED_SENTENCE,
        "The Browser is not shared for this session: open it in the Beekeeper app with Share on."
    );
}
