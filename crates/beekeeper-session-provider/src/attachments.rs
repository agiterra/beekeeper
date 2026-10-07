//! Fetch a turn's attachments and render them as ACP prompt blocks.
//!
//! The operator uploads to the relay's Blossom store and the 44220 command
//! carries only the blob's hash; this module is the other half — it derives the
//! URL from the relay *this provider* is connected to, reads the bytes back
//! under a signed `t=get` token **and the provider's membership delegation**,
//! and bounds what reaches a model.
//!
//! Addressing by hash rather than URL is deliberate. A URL in the payload would
//! let whoever signed the command choose where this process makes an outbound
//! request; a hash cannot.
//!
//! Two kinds travel this path and the difference is not cosmetic. An **image**
//! becomes an ACP `image` block, is bounded in pixels, and needs a runtime that
//! advertised `promptImage`. **Text** — what a large paste in the composer
//! becomes — becomes an ACP `text` block, is bounded in bytes, and needs no
//! capability at all: a turn is already text. Text exists on this path for one
//! reason: a turn's own `action.text` is capped at 12 KiB, so an arbitrarily
//! long log, diff or stack trace can only reach the agent as a blob.

use base64::Engine;
use beekeeper_acp::acp::PromptBlock;
use beekeeper_core::coding_session_command::TurnAttachment;
use image::{codecs::png::PngEncoder, DynamicImage, ImageEncoder, ImageReader, Limits};
use sha2::{Digest, Sha256};
use std::io::Cursor;
use std::time::Duration;

/// Lifetime of a Blossom `t=get` read token. Matches the desktop client's
/// `MEDIA_GET_AUTH_EXPIRY_SECS` and `beekeeper-dev-mcp`'s `view_image`.
const MEDIA_GET_AUTH_EXPIRY_SECS: u64 = 600;
/// Connect + read timeout for one blob fetch.
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
/// Longest-edge cap applied before encoding. Anthropic's published
/// recommendation, and well inside OpenAI's high-detail tile budget.
const MAX_DIM: u32 = 1568;
/// Hard cap on decoded pixel count, checked from the header before the decoder
/// allocates: a small compressed file can decode to hundreds of megabytes.
const MAX_PIXELS: u64 = 64 * 1024 * 1024;
/// Defence-in-depth cap on any single decoder allocation.
const MAX_DECODER_ALLOC: u64 = 256 * 1024 * 1024;
/// Hard cap on the bytes of one text attachment that reach a prompt.
///
/// The same ceiling the command contract validates, re-applied to the bytes
/// actually read back, because the two are not the same claim: `size` in the
/// payload is what the signer *said*, and the hash check proves only that the
/// blob is the one they meant — not that it is as small as they declared. Every
/// byte here is delivered verbatim into a context window, so this is the one
/// bound standing between a signed command and a prompt the model cannot hold.
const MAX_TEXT_BYTES: u64 = beekeeper_core::coding_session_command::MAX_TURN_TEXT_ATTACHMENT_BYTES;

/// Everything the fetcher needs, captured when the session actor is built so a
/// turn never reaches back into global state.
#[derive(Clone)]
pub struct MediaFetcher {
    http: reqwest::Client,
    /// `https://host[:port]` derived from the configured relay URL.
    base: String,
    /// `host[:port]`, for the Blossom `server` tag.
    authority: String,
    keys: nostr::Keys,
    /// The provider's NIP-OA auth tag as JSON, for the `x-auth-tag` header.
    ///
    /// Load-bearing on any relay that requires membership. The Blossom token
    /// proves *which key* is asking; this header is how that key's delegation
    /// from its owner travels over HTTP, and the provider's own pubkey is not
    /// a member in its own right — it is admitted through the owner. Without
    /// it the relay answers `403 relay_membership_required`, every attachment
    /// is dropped, and the agent is left with the markdown link the prose
    /// carries, which it cannot read either. `ArtifactUploader` carries the
    /// same header for the same reason.
    auth_tag_json: Option<String>,
}

impl MediaFetcher {
    /// Build a fetcher for the relay this provider is configured against.
    ///
    /// `auth_tag` is the provider's NIP-OA credential, which must be passed
    /// wherever the config has one: a membership-gated relay refuses the read
    /// without it. It is optional only because an open relay has no use for
    /// one, and because the config's own field is optional.
    ///
    /// Returns `None` when the relay URL cannot be understood, which simply
    /// means attachments are not deliverable on this host — never a panic on a
    /// path a turn depends on.
    pub fn new(relay_url: &str, keys: nostr::Keys, auth_tag: Option<&nostr::Tag>) -> Option<Self> {
        let authority = beekeeper_core::tenant::relay_url_authority(relay_url);
        if authority.is_empty() {
            return None;
        }
        // `ws://`/`wss://` are the configured forms; media is served over the
        // matching HTTP scheme on the same authority.
        let scheme = if relay_url.starts_with("ws://") || relay_url.starts_with("http://") {
            "http"
        } else {
            "https"
        };
        Some(Self {
            http: reqwest::Client::builder()
                .timeout(FETCH_TIMEOUT)
                // Do not follow redirects. The module's premise is that a hash
                // cannot choose where this process makes an outbound request;
                // a followed redirect would hand that choice back, and would
                // forward `x-auth-tag` with it — reqwest strips
                // `Authorization` across hosts but not that header. The relay
                // streams blob bytes directly, so there is nothing to follow.
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .ok()?,
            base: format!("{scheme}://{authority}"),
            authority,
            keys,
            auth_tag_json: auth_tag.and_then(|tag| serde_json::to_string(tag).ok()),
        })
    }

    /// Fetch every attachment and render it as an ACP prompt block.
    ///
    /// Attachments that cannot be fetched or decoded are skipped with a warning
    /// rather than failing the turn: the operator's words are the turn, and
    /// losing them because one blob 404'd would be the worse outcome. The count
    /// actually delivered is returned so the caller can tell the truth about it.
    /// Concurrently, and in the operator's order. Concurrency is not a
    /// micro-optimisation here: this runs before the turn's opening transcript
    /// item is emitted, so four sequential fetches would each add their own
    /// timeout to the delay before anyone sees the turn start.
    pub async fn attachment_blocks(
        &self,
        attachments: &[TurnAttachment],
    ) -> Vec<(String, PromptBlock)> {
        let mut tasks = tokio::task::JoinSet::new();
        for (index, attachment) in attachments.iter().enumerate() {
            let fetcher = self.clone();
            let attachment = attachment.clone();
            tasks.spawn(async move {
                let result = fetcher.attachment_block(&attachment).await;
                (index, attachment.sha256, result)
            });
        }

        let mut slots: Vec<Option<(String, PromptBlock)>> = vec![None; attachments.len()];
        while let Some(joined) = tasks.join_next().await {
            match joined {
                Ok((index, sha256, Ok(block))) => slots[index] = Some((sha256, block)),
                Ok((_, sha256, Err(error))) => tracing::warn!(
                    target: "csp::attachments",
                    "dropping attachment {sha256}: {error}"
                ),
                Err(error) => tracing::warn!(
                    target: "csp::attachments",
                    "attachment fetch task failed: {error}"
                ),
            }
        }
        // Order is the operator's, with failures closed over rather than left
        // as holes: "the second image" in their prompt has to mean the second
        // image the agent received.
        slots.into_iter().flatten().collect()
    }

    /// Read one attachment back and render it as the block its kind calls for.
    ///
    /// The kind is decided by the MIME the *command* declared, which the relay
    /// has already checked against its allowlist. The bytes are then held to
    /// that claim — text that is not UTF-8 is refused rather than lossily
    /// coerced — so a declared kind can never make this process treat bytes as
    /// something they are not.
    async fn attachment_block(&self, attachment: &TurnAttachment) -> Result<PromptBlock, String> {
        let bytes = self.fetch_blob(attachment).await?;
        if attachment.is_text() {
            return text_block(attachment, &bytes);
        }
        let (mime, encoded) = downscale_to_png(&bytes)?;
        Ok(PromptBlock::Image {
            mime,
            data_base64: base64::engine::general_purpose::STANDARD.encode(encoded),
        })
    }

    /// Fetch the blob this attachment names and prove it is the one declared.
    async fn fetch_blob(&self, attachment: &TurnAttachment) -> Result<Vec<u8>, String> {
        let url = format!(
            "{}/media/{}.{}",
            self.base,
            attachment.sha256,
            attachment.extension()
        );
        let authorization = self.sign_get_auth()?;
        let mut request = self
            .http
            .get(&url)
            .header(reqwest::header::AUTHORIZATION, authorization);
        if let Some(json) = &self.auth_tag_json {
            request = request.header("x-auth-tag", json);
        }
        let response = request
            .send()
            .await
            .map_err(|error| format!("fetch failed: {error}"))?;
        if !response.status().is_success() {
            return Err(format!("relay answered {}", response.status()));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| format!("read failed: {error}"))?;

        // The command declared this hash; the relay is content-addressed, so a
        // mismatch means we are about to show the agent something other than
        // what the operator attached.
        let digest = Sha256::digest(&bytes);
        if hex::encode(digest) != attachment.sha256 {
            return Err("fetched bytes do not match the declared sha256".into());
        }
        Ok(bytes.to_vec())
    }

    /// Sign a Blossom (BUD-01) `t=get` token scoped to this relay's authority.
    fn sign_get_auth(&self) -> Result<String, String> {
        use nostr::{EventBuilder, JsonUtil, Kind, Tag, Timestamp};
        let now = Timestamp::now().as_secs();
        let tags = vec![
            Tag::parse(["t", "get"]).map_err(|error| error.to_string())?,
            Tag::parse([
                "expiration",
                &(now + MEDIA_GET_AUTH_EXPIRY_SECS).to_string(),
            ])
            .map_err(|error| error.to_string())?,
            Tag::parse(["server", self.authority.as_str()]).map_err(|error| error.to_string())?,
        ];
        let event = EventBuilder::new(Kind::from(24242), "Get buzz-media")
            .tags(tags)
            .sign_with_keys(&self.keys)
            .map_err(|error| error.to_string())?;
        Ok(format!(
            "Nostr {}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(event.as_json().as_bytes())
        ))
    }
}

/// Render a text attachment as the ACP `text` block the agent receives.
///
/// Three jobs, in this order, and each one is a refusal the caller turns into a
/// dropped attachment the agent is then told about:
///
/// 1. **Bound it.** [`MAX_TEXT_BYTES`] against the bytes actually read, not the
///    size the command declared — see that constant.
/// 2. **Prove it is text.** Invalid UTF-8 is refused rather than replaced with
///    `U+FFFD`: a lossy transcode would hand the agent a file that silently
///    differs from the one the person pasted, and "this did not arrive" is a
///    better answer than a corrupted one.
/// 3. **Frame it.** A header naming the file, its line count and its size, then
///    the content inside a fence long enough that nothing in the content can
///    close it. Without a fence an agent cannot tell where a pasted log ends
///    and the operator's next instruction begins, and a log that happens to
///    contain a bare ``` would do exactly that.
fn text_block(attachment: &TurnAttachment, bytes: &[u8]) -> Result<PromptBlock, String> {
    if bytes.len() as u64 > MAX_TEXT_BYTES {
        return Err(format!(
            "text attachment is {} bytes, over the {MAX_TEXT_BYTES}-byte bound",
            bytes.len()
        ));
    }
    let content = std::str::from_utf8(bytes)
        .map_err(|error| format!("text attachment is not valid UTF-8: {error}"))?;
    let name = display_filename(attachment);
    let lines = content.lines().count();
    let plural = if lines == 1 { "line" } else { "lines" };
    let fence = "`".repeat(fence_width(content));
    Ok(PromptBlock::Text(format!(
        "[Attached text file: {name} — {lines} {plural}, {} bytes]\n{fence}\n{}\n{fence}",
        bytes.len(),
        content.trim_end_matches('\n')
    )))
}

/// The filename to show the agent, bounded and stripped of anything that could
/// break the one-line header it sits in.
///
/// Operator-supplied and display-only, so it is never trusted as a path: no
/// directory separators survive, control characters and backticks are dropped,
/// and the whole thing is cut to a length a header can carry. An attachment
/// with no filename is named by its blob instead of by a guess.
fn display_filename(attachment: &TurnAttachment) -> String {
    const MAX_NAME_CHARS: usize = 80;
    let raw = attachment.filename.as_deref().unwrap_or("");
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '`' | '/' | '\\'))
        .take(MAX_NAME_CHARS)
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        // By characters, not by a byte slice: the relay validates `sha256` as
        // 64 lowercase hex, but this module must not be the one that panics if
        // it ever sees something else.
        let stem: String = attachment.sha256.chars().take(8).collect();
        format!("{stem}.txt")
    } else {
        cleaned.to_owned()
    }
}

/// How many backticks the fence around `content` needs.
///
/// One more than the longest run inside it, never fewer than three. This is the
/// CommonMark rule for the same reason it exists there: a fence shorter than a
/// run in the body is closed by the body.
fn fence_width(content: &str) -> usize {
    let mut longest = 0usize;
    let mut run = 0usize;
    for ch in content.chars() {
        if ch == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    longest.saturating_add(1).max(3)
}

/// What the agent is told when an attachment did not arrive, if any did not.
///
/// The prose still carries every image's `![image](…/media/<sha>.<ext>)`
/// reference — [`interleave_prompt_blocks`] deliberately does not rewrite text
/// the operator signed — so an agent handed the words without the picture sees
/// a link and, left to itself, fetches it. That request carries no Blossom
/// token, the relay answers `401`, and the agent reports *that* as the reason
/// the image is missing. It is not the reason. The person who pasted the
/// screenshot is then asked to work around a status code that describes only
/// the agent's own unauthorized request, and the actual failure — a `403` in
/// this process, already in the host log — is never mentioned.
///
/// So say it plainly: this many did not arrive, the link is not a way to get
/// them, and the honest answer is to ask for them again. The transcript
/// already counts what was delivered (`begin_turn`'s `attachment_count`);
/// this is the same fact told to the one party that has to act on it.
///
/// A dropped *text* attachment is the worse of the two cases, because the words
/// are the content: a pasted 400-line log that did not arrive leaves the agent
/// a one-line link where the operator believes it has the whole file. So the
/// note names the kind rather than saying "image" over a missing paste.
pub(crate) fn undelivered_note(attachments: &[TurnAttachment], delivered: usize) -> Option<String> {
    let attached = attachments.len();
    let missing = attached.checked_sub(delivered).filter(|count| *count > 0)?;
    let all_images = attachments.iter().all(TurnAttachment::is_image);
    let all_text = attachments.iter().all(TurnAttachment::is_text);
    // The noun agrees with the total attached; the verb and the pronoun agree
    // with how many are missing. "1 of the 2 image" was the first draft.
    let (singular, link_kind) = match (all_images, all_text) {
        (true, false) => ("image", "image "),
        (false, true) => ("text attachment", ""),
        // Mixed, or an empty list this function already returned `None` for.
        _ => ("attachment", ""),
    };
    let plural = if attached == 1 {
        singular.to_owned()
    } else {
        format!("{singular}s")
    };
    let (is_are, them) = if missing == 1 {
        ("is", "it")
    } else {
        ("are", "them")
    };
    Some(format!(
        "[Attachments]\n{missing} of the {attached} {plural} attached to this message could not \
         be read back from the relay and {is_are} not part of this turn. The markdown {link_kind}link \
         in the text above is not a way to get {them}: an unauthenticated request to the relay's \
         media store answers 401, which says nothing about why the {singular} is missing. Say the \
         {singular} did not reach you and ask for it again, rather than reporting a status from that \
         link as the cause."
    ))
}

/// Interleave fetched attachments into the prompt at the positions the operator
/// wrote them.
///
/// The turn text carries one markdown reference per attachment — an image link
/// (`![image](…/media/<sha>.png)`), which is also what makes the picture render
/// in the transcript, or a plain link (`[pasted-text-1.txt](…/media/<sha>.txt)`)
/// for a text file. Splitting on those references means the agent reads *"when I
/// do X I see this:"*, then the screenshot, then *"but I want to see:"*, then
/// the next one — the same order a person reading the transcript sees. Appending
/// everything after the prose instead loses which sentence each one belongs to,
/// which for a two-screenshot before/after turn is the entire meaning, and for a
/// pasted log is the difference between "fix this" and a wall of text.
///
/// Attachments whose reference is absent from the text are appended rather than
/// dropped: a lost marker must not silently cost the agent its content.
pub(crate) fn interleave_prompt_blocks(
    text: &str,
    mut attachments: Vec<(String, PromptBlock)>,
) -> Vec<PromptBlock> {
    let mut blocks = Vec::new();
    let mut rest = text;

    while let Some((before, sha, after)) = split_at_attachment_reference(rest, &attachments) {
        push_text(&mut blocks, before);
        if let Some(index) = attachments
            .iter()
            .position(|(candidate, _)| *candidate == sha)
        {
            blocks.push(attachments.remove(index).1);
        }
        rest = after;
    }
    push_text(&mut blocks, rest);

    // Anything the prose never referenced still reaches the agent, after the
    // words, which is where an unpositioned attachment belongs.
    blocks.extend(attachments.into_iter().map(|(_, block)| block));
    blocks
}

/// Push a text block unless it is only whitespace.
fn push_text(blocks: &mut Vec<PromptBlock>, text: &str) {
    let trimmed = text.trim();
    if !trimmed.is_empty() {
        blocks.push(PromptBlock::Text(trimmed.to_owned()));
    }
}

/// Find the next markdown link, image or plain, whose url names one of
/// `attachments`.
///
/// Hand-rolled rather than a regex dependency: the shape is fixed and the only
/// thing that matters is the 64-hex blob id inside the parentheses. Scanning
/// from `](` rather than from `![` is what makes one function serve both forms —
/// an image reference is a plain one with a `!` in front, so the opening bracket
/// is found by walking back and the `!` is swept up with it.
///
/// A reference to an attachment this turn did not carry is left in the prose,
/// because rewriting text the operator signed is not this function's business.
/// A link label containing its own `[` keeps that bracket in the prose rather
/// than in the label — harmless, and the alternative is matching brackets in
/// text a person wrote by hand.
fn split_at_attachment_reference<'a>(
    text: &'a str,
    attachments: &[(String, PromptBlock)],
) -> Option<(&'a str, String, &'a str)> {
    let mut cursor = 0usize;
    while let Some(offset) = text[cursor..].find("](") {
        let close = cursor + offset;
        let url_start = close + 2;
        let Some(end) = text[url_start..].find(')').map(|offset| url_start + offset) else {
            break;
        };
        let url = &text[url_start..end];
        if let Some(sha) = attachments
            .iter()
            .map(|(sha, _)| sha)
            .find(|sha| url.contains(sha.as_str()))
        {
            let open = text[..close].rfind('[').unwrap_or(close);
            // `!` is one byte, and `open` is the index of a `[`, so both of
            // these are char boundaries.
            let open = if text[..open].ends_with('!') {
                open - 1
            } else {
                open
            };
            return Some((&text[..open], sha.clone(), &text[end + 1..]));
        }
        cursor = end + 1;
    }
    None
}

/// Decode, bound the longest edge to [`MAX_DIM`], and re-encode as PNG.
///
/// Always PNG: one output format means one code path, and re-encoding rather
/// than passing the source bytes through also strips whatever metadata the
/// original carried before it reaches a model provider.
pub(crate) fn downscale_to_png(bytes: &[u8]) -> Result<(String, Vec<u8>), String> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| format!("unreadable image: {error}"))?;
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_DECODER_ALLOC);
    reader.limits(limits);

    // Check the header's declared size before handing the decoder the body.
    if let Ok((width, height)) = reader.into_dimensions() {
        if u64::from(width) * u64::from(height) > MAX_PIXELS {
            return Err(format!("image is too large to decode: {width}x{height}"));
        }
    }
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| format!("unreadable image: {error}"))?;
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_DECODER_ALLOC);
    reader.limits(limits);
    let decoded = reader
        .decode()
        .map_err(|error| format!("undecodable image: {error}"))?;

    let decoded = if decoded.width() > MAX_DIM || decoded.height() > MAX_DIM {
        decoded.resize(MAX_DIM, MAX_DIM, image::imageops::FilterType::Triangle)
    } else {
        decoded
    };
    // Flatten to 8-bit RGBA so the encoder never has to reason about the
    // source's colour type.
    let rgba = DynamicImage::ImageRgba8(decoded.to_rgba8());
    let mut out = Vec::new();
    PngEncoder::new(&mut out)
        .write_image(
            rgba.as_bytes(),
            rgba.width(),
            rgba.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| format!("re-encode failed: {error}"))?;
    Ok(("image/png".to_owned(), out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        let img = DynamicImage::ImageRgba8(image::RgbaImage::new(width, height));
        let mut out = Vec::new();
        img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .expect("encode");
        out
    }

    /// An image inside the budget keeps its dimensions; an oversized one is
    /// bounded to the longest-edge cap rather than refused.
    #[test]
    fn downscale_bounds_only_oversized_images() {
        let (mime, small) = downscale_to_png(&png_bytes(64, 32)).expect("small");
        assert_eq!(mime, "image/png");
        let decoded = image::load_from_memory(&small).expect("decode");
        assert_eq!((decoded.width(), decoded.height()), (64, 32));

        let (_, big) = downscale_to_png(&png_bytes(4000, 2000)).expect("big");
        let decoded = image::load_from_memory(&big).expect("decode");
        assert_eq!(decoded.width(), MAX_DIM);
        assert!(decoded.height() <= MAX_DIM);
    }

    /// Bytes that are not an image are rejected, never passed through.
    #[test]
    fn non_image_bytes_are_refused() {
        assert!(downscale_to_png(b"this is not an image at all").is_err());
    }

    /// A relay URL we cannot parse yields no fetcher rather than a panic.
    #[test]
    fn an_unusable_relay_url_yields_no_fetcher() {
        assert!(MediaFetcher::new("", nostr::Keys::generate(), None).is_none());
    }

    /// The `ws`/`wss` the provider is configured with maps to the matching
    /// HTTP scheme on the same authority — media is not served over WebSocket.
    #[test]
    fn media_base_follows_the_relay_scheme() {
        let secure = MediaFetcher::new("wss://hive.example", nostr::Keys::generate(), None)
            .expect("fetcher");
        assert_eq!(secure.base, "https://hive.example");
        let plain = MediaFetcher::new("ws://127.0.0.1:3000", nostr::Keys::generate(), None)
            .expect("fetcher");
        assert_eq!(plain.base, "http://127.0.0.1:3000");
    }

    fn described(mime: &str, sha: char) -> TurnAttachment {
        TurnAttachment {
            sha256: std::iter::repeat_n(sha, 64).collect(),
            mime: mime.into(),
            size: 1024,
            dim: None,
            filename: None,
        }
    }

    fn pngs(count: usize) -> Vec<TurnAttachment> {
        ('a'..)
            .take(count)
            .map(|c| described("image/png", c))
            .collect()
    }

    fn texts(count: usize) -> Vec<TurnAttachment> {
        ('a'..)
            .take(count)
            .map(|c| described("text/plain", c))
            .collect()
    }

    /// Every attachment delivered means nothing to say; one missing means the
    /// agent is told, in the numbers, and told not to chase the link.
    #[test]
    fn an_undelivered_attachment_is_disclosed_to_the_agent() {
        assert_eq!(undelivered_note(&[], 0), None);
        assert_eq!(undelivered_note(&pngs(2), 2), None);
        // Cannot happen, and must not underflow into a note claiming a
        // negative number of missing images.
        assert_eq!(undelivered_note(&pngs(1), 2), None);

        let one = undelivered_note(&pngs(2), 1).expect("one image is missing");
        assert!(one.starts_with("[Attachments]\n"), "{one}");
        assert!(one.contains("1 of the 2 images"), "{one}");
        assert!(one.contains("is not part of this turn"), "{one}");
        // The two sentences this note exists for: the link is not a route to
        // the image, and the 401 it would answer is not the explanation.
        assert!(one.contains("not a way to get it"), "{one}");
        assert!(one.contains("401"), "{one}");
        assert!(one.contains("ask for it again"), "{one}");

        // The noun agrees with the total, not with the missing count.
        let single = undelivered_note(&pngs(1), 0).expect("the only image is missing");
        assert!(single.contains("1 of the 1 image attached"), "{single}");
        assert!(single.contains("is not part of this turn"), "{single}");

        let both = undelivered_note(&pngs(2), 0).expect("both images are missing");
        assert!(both.contains("2 of the 2 images"), "{both}");
        assert!(both.contains("are not part of this turn"), "{both}");
        assert!(both.contains("not a way to get them"), "{both}");
    }

    /// The note names the kind that went missing. Saying "image" over a dropped
    /// paste would send the agent looking for a picture that was never there,
    /// and a dropped paste is the case where the content *is* the words.
    #[test]
    fn the_note_names_the_kind_that_did_not_arrive() {
        let text = undelivered_note(&texts(1), 0).expect("the paste is missing");
        assert!(
            text.contains("1 of the 1 text attachment attached"),
            "{text}"
        );
        assert!(
            text.contains("why the text attachment is missing"),
            "{text}"
        );
        // No "image link": a text attachment is referenced by a plain link.
        assert!(!text.contains("image"), "{text}");

        let several = undelivered_note(&texts(3), 1).expect("two pastes are missing");
        assert!(several.contains("2 of the 3 text attachments"), "{several}");

        // Mixed turns fall back to the kind-neutral noun rather than picking
        // one of the two and being wrong about the other.
        let mixed = vec![described("image/png", 'a'), described("text/plain", 'b')];
        let note = undelivered_note(&mixed, 0).expect("both are missing");
        assert!(note.contains("2 of the 2 attachments"), "{note}");
        assert!(!note.contains("image"), "{note}");
    }

    /// A text attachment arrives as a framed `text` block: a header naming the
    /// file and its shape, then the content, fenced.
    #[test]
    fn a_text_attachment_becomes_a_framed_text_block() {
        let attachment = TurnAttachment {
            filename: Some("stack-trace.txt".into()),
            ..described("text/plain", 'a')
        };
        let content = "line one\nline two\nline three\n";
        let block = text_block(&attachment, content.as_bytes()).expect("text block");
        let rendered = text_of(&block).expect("a text block");
        assert!(
            rendered
                .starts_with("[Attached text file: stack-trace.txt — 3 lines, 29 bytes]\n```\n"),
            "{rendered}"
        );
        assert!(rendered.contains("line two"), "{rendered}");
        assert!(rendered.ends_with("line three\n```"), "{rendered}");
        // One line is "1 line", not "1 lines".
        let one = text_block(&attachment, b"just this").expect("one line");
        assert!(
            text_of(&one).expect("text").contains("1 line, 9 bytes"),
            "{one:?}"
        );
    }

    /// Content carrying its own fence must not be able to close the one around
    /// it, or everything after the paste reads as the operator's instruction.
    #[test]
    fn the_fence_outgrows_any_run_of_backticks_in_the_content() {
        assert_eq!(fence_width("plain"), 3);
        assert_eq!(fence_width("a ``` b"), 4);
        assert_eq!(fence_width("`````"), 6);

        let attachment = described("text/plain", 'a');
        let block =
            text_block(&attachment, b"before\n```\nnot the end\n```\nafter").expect("text block");
        let rendered = text_of(&block).expect("text");
        assert!(rendered.contains("````\nbefore"), "{rendered}");
        assert!(rendered.ends_with("after\n````"), "{rendered}");
    }

    /// Two refusals, both of which become "this did not arrive" rather than a
    /// corrupted or unbounded prompt: bytes that are not UTF-8, and bytes over
    /// the ceiling whatever the command declared its size to be.
    #[test]
    fn text_that_is_not_text_or_is_too_large_is_refused() {
        let attachment = described("text/plain", 'a');
        let error = text_block(&attachment, &[0xff, 0xfe, 0x41]).expect_err("not utf-8");
        assert!(error.contains("not valid UTF-8"), "{error}");

        // The command said 1024 bytes; the blob is over the bound. The hash
        // check cannot catch this, so the byte count has to.
        let huge = vec![b'x'; (MAX_TEXT_BYTES + 1) as usize];
        let error = text_block(&attachment, &huge).expect_err("over the bound");
        assert!(error.contains(&MAX_TEXT_BYTES.to_string()), "{error}");
        let at_limit = vec![b'x'; MAX_TEXT_BYTES as usize];
        assert!(
            text_block(&attachment, &at_limit).is_ok(),
            "the bound is inclusive"
        );
    }

    /// The filename is the operator's and display-only, so it never reaches the
    /// header as a path, a control character, or something that could close the
    /// fence. An attachment with no filename is named by its blob.
    #[test]
    fn the_displayed_filename_cannot_break_the_header() {
        let named = |name: Option<&str>| {
            display_filename(&TurnAttachment {
                filename: name.map(str::to_owned),
                ..described("text/plain", 'a')
            })
        };
        assert_eq!(named(Some("notes.txt")), "notes.txt");
        assert_eq!(named(Some("../../etc/passwd")), "....etcpasswd");
        assert_eq!(named(Some("one\nline```")), "oneline");
        assert_eq!(named(None), "aaaaaaaa.txt");
        assert_eq!(named(Some("   ")), "aaaaaaaa.txt");
        assert_eq!(named(Some(&"n".repeat(200))).len(), 80);
    }

    /// The whole read path against a real HTTP server: the URL is derived from
    /// the relay and the hash, the request carries a Blossom `t=get` token
    /// **and the provider's membership delegation**, and the answer becomes an
    /// ACP image block.
    #[tokio::test]
    async fn a_blob_is_fetched_by_hash_and_becomes_an_image_block() {
        use axum::{extract::State, routing::get, Router};
        use std::sync::{Arc, Mutex};

        /// Requests the stub server saw: path, `Authorization`, `x-auth-tag`.
        type Seen = Arc<Mutex<Vec<(String, Option<String>, Option<String>)>>>;

        let png = png_bytes(32, 16);
        let sha = hex::encode(Sha256::digest(&png));
        // What the server was asked for, so the test can assert the path and
        // the auth header rather than only the happy-path bytes.
        let seen: Seen = Arc::new(Mutex::new(Vec::new()));

        async fn serve(
            State((body, seen)): State<(Arc<Vec<u8>>, Seen)>,
            request: axum::extract::Request,
        ) -> Vec<u8> {
            let header = |name: &str| {
                request
                    .headers()
                    .get(name)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned)
            };
            let authorization = header("authorization");
            let auth_tag = header("x-auth-tag");
            seen.lock().expect("lock").push((
                request.uri().path().to_owned(),
                authorization,
                auth_tag,
            ));
            body.as_ref().clone()
        }

        let app = Router::new()
            .route("/media/{blob}", get(serve))
            .with_state((Arc::new(png.clone()), seen.clone()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });

        let auth_tag =
            nostr::Tag::parse(["owner-attestation", "payload"]).expect("a NIP-OA shaped tag");
        let fetcher = MediaFetcher::new(
            &format!("ws://{address}"),
            nostr::Keys::generate(),
            Some(&auth_tag),
        )
        .expect("fetcher");
        let attachment = TurnAttachment {
            sha256: sha.clone(),
            mime: "image/png".into(),
            size: png.len() as u64,
            dim: Some("32x16".into()),
            filename: Some("shot.png".into()),
        };

        let blocks = fetcher
            .attachment_blocks(std::slice::from_ref(&attachment))
            .await;
        assert_eq!(blocks.len(), 1, "the blob should have become one block");
        assert_eq!(blocks[0].0, sha, "each block is tagged with its blob id");
        match &blocks[0].1 {
            PromptBlock::Image { mime, data_base64 } => {
                assert_eq!(mime, "image/png");
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(data_base64)
                    .expect("base64");
                let image = image::load_from_memory(&decoded).expect("decode");
                assert_eq!((image.width(), image.height()), (32, 16));
            }
            other => panic!("expected an image block, got {other:?}"),
        }

        let seen = seen.lock().expect("lock").clone();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].0, format!("/media/{sha}.png"));
        assert!(
            seen[0]
                .1
                .as_deref()
                .is_some_and(|value| value.starts_with("Nostr ")),
            "relay media reads are authenticated: {:?}",
            seen[0].1
        );
        // The Blossom token proves *which key* is asking; this header is how
        // that key's delegation from its owner travels. A membership-gated
        // relay answers `403 relay_membership_required` without it, which is
        // what shipped — every pasted screenshot was dropped, and the agent
        // was left chasing the markdown link in the prose.
        assert_eq!(
            seen[0].2.as_deref(),
            Some(r#"["owner-attestation","payload"]"#),
            "the provider's membership delegation must reach the relay"
        );

        // A blob whose bytes do not match the hash the command declared is
        // refused: showing the agent something other than what the operator
        // attached is worse than showing it nothing.
        let tampered = TurnAttachment {
            sha256: "0".repeat(64),
            ..attachment
        };
        assert!(
            fetcher.attachment_blocks(&[tampered]).await.is_empty(),
            "a hash mismatch must drop the attachment"
        );

        server.abort();
    }

    /// Concurrency must not reorder the operator's images, and a failure in
    /// the middle must close over rather than leave a hole: "the second image"
    /// in a prompt has to mean the second image the agent actually received.
    #[tokio::test]
    async fn results_keep_operator_order_across_a_middle_failure() {
        use axum::{extract::Path, routing::get, Router};

        let wide = png_bytes(40, 10);
        let tall = png_bytes(10, 40);
        let wide_sha = hex::encode(Sha256::digest(&wide));
        let tall_sha = hex::encode(Sha256::digest(&tall));

        let bodies = std::sync::Arc::new(vec![
            (wide_sha.clone(), wide.clone()),
            (tall_sha.clone(), tall.clone()),
        ]);
        let app = Router::new().route(
            "/media/{blob}",
            get({
                let bodies = bodies.clone();
                move |Path(blob): Path<String>| {
                    let bodies = bodies.clone();
                    async move {
                        let stem = blob.split('.').next().unwrap_or_default().to_owned();
                        match bodies.iter().find(|(sha, _)| *sha == stem) {
                            Some((_, body)) => body.clone(),
                            // The middle attachment is served bytes that do not
                            // match its declared hash.
                            None => b"not the declared blob".to_vec(),
                        }
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });

        let fetcher = MediaFetcher::new(&format!("ws://{address}"), nostr::Keys::generate(), None)
            .expect("fetcher");
        let describe = |sha: &str, size: usize| TurnAttachment {
            sha256: sha.to_owned(),
            mime: "image/png".into(),
            size: size as u64,
            dim: None,
            filename: None,
        };

        let blocks = fetcher
            .attachment_blocks(&[
                describe(&wide_sha, wide.len()),
                describe(&"9".repeat(64), 10),
                describe(&tall_sha, tall.len()),
            ])
            .await;

        assert_eq!(blocks.len(), 2, "the unfetchable attachment is dropped");
        let dimensions: Vec<(u32, u32)> = blocks
            .iter()
            .map(|(_, block)| match block {
                PromptBlock::Image { data_base64, .. } => {
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(data_base64)
                        .expect("base64");
                    let image = image::load_from_memory(&bytes).expect("decode");
                    (image.width(), image.height())
                }
                other => panic!("expected an image block, got {other:?}"),
            })
            .collect();
        assert_eq!(
            dimensions,
            vec![(40, 10), (10, 40)],
            "the surviving images keep the order they were attached in"
        );

        server.abort();
    }

    fn image(tag: &str) -> PromptBlock {
        PromptBlock::Image {
            mime: "image/png".into(),
            data_base64: tag.to_owned(),
        }
    }

    /// A pasted file lands where the operator wrote it, exactly as an image
    /// does, even though its reference is a plain link rather than an image one.
    #[test]
    fn a_text_reference_lands_where_the_operator_wrote_it() {
        let a = "a".repeat(64);
        let text =
            format!("Here is the log:\n\n[paste.txt](http://relay/media/{a}.txt)\n\nWhat broke?");
        let blocks = interleave_prompt_blocks(
            &text,
            vec![(
                a,
                PromptBlock::Text("[Attached text file: paste.txt]".into()),
            )],
        );
        assert_eq!(blocks.len(), 3);
        assert_eq!(text_of(&blocks[0]), Some("Here is the log:"));
        assert_eq!(text_of(&blocks[1]), Some("[Attached text file: paste.txt]"));
        assert_eq!(text_of(&blocks[2]), Some("What broke?"));
    }

    /// One turn, both kinds, each at its own position — a screenshot and the log
    /// that goes with it is the ordinary case, not an exotic one.
    #[test]
    fn an_image_and_a_paste_interleave_in_one_turn() {
        let img = "a".repeat(64);
        let txt = "b".repeat(64);
        let text = format!(
            "I see ![image](http://relay/media/{img}.png) and the log is [paste.txt](http://relay/media/{txt}.txt) — why?"
        );
        let blocks = interleave_prompt_blocks(
            &text,
            vec![
                (img, image("shot")),
                (txt, PromptBlock::Text("the log".into())),
            ],
        );
        assert_eq!(blocks.len(), 5);
        assert_eq!(text_of(&blocks[0]), Some("I see"));
        assert_eq!(data_of(&blocks[1]), Some("shot"));
        assert_eq!(text_of(&blocks[2]), Some("and the log is"));
        assert_eq!(text_of(&blocks[3]), Some("the log"));
        assert_eq!(text_of(&blocks[4]), Some("— why?"));
    }

    fn text_of(block: &PromptBlock) -> Option<&str> {
        match block {
            PromptBlock::Text(text) => Some(text.as_str()),
            PromptBlock::Image { .. } => None,
        }
    }

    fn data_of(block: &PromptBlock) -> Option<&str> {
        match block {
            PromptBlock::Image { data_base64, .. } => Some(data_base64.as_str()),
            PromptBlock::Text(_) => None,
        }
    }

    /// The before/after turn this feature exists for: two screenshots, each
    /// belonging to the sentence above it. Appending both after the prose
    /// would lose which one is "what I see" and which is "what I want".
    #[test]
    fn images_land_where_the_operator_wrote_them() {
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        let text = format!(
            "When I do X, I see this:\n\n![image](http://relay/media/{a}.png)\n\nBut I want to see:\n\n![image](http://relay/media/{b}.png)"
        );
        let blocks = interleave_prompt_blocks(
            &text,
            vec![(a.clone(), image("first")), (b.clone(), image("second"))],
        );

        assert_eq!(blocks.len(), 4);
        assert_eq!(text_of(&blocks[0]), Some("When I do X, I see this:"));
        assert_eq!(data_of(&blocks[1]), Some("first"));
        assert_eq!(text_of(&blocks[2]), Some("But I want to see:"));
        assert_eq!(data_of(&blocks[3]), Some("second"));
    }

    /// An image the prose never referenced still reaches the agent, after the
    /// words. Losing a marker must cost position, never the picture.
    #[test]
    fn an_unreferenced_image_is_appended_rather_than_dropped() {
        let a = "a".repeat(64);
        let blocks = interleave_prompt_blocks("just words", vec![(a, image("orphan"))]);
        assert_eq!(blocks.len(), 2);
        assert_eq!(text_of(&blocks[0]), Some("just words"));
        assert_eq!(data_of(&blocks[1]), Some("orphan"));
    }

    /// A reference to something this turn did not carry is left alone — the
    /// operator signed that text, and rewriting it is not ours to do.
    #[test]
    fn an_unknown_image_reference_stays_in_the_prose() {
        let a = "a".repeat(64);
        let text = format!(
            "see ![image](http://elsewhere/x.png) and ![image](http://relay/media/{a}.png)"
        );
        let blocks = interleave_prompt_blocks(&text, vec![(a, image("mine"))]);
        assert_eq!(blocks.len(), 2);
        assert_eq!(
            text_of(&blocks[0]),
            Some("see ![image](http://elsewhere/x.png) and")
        );
        assert_eq!(data_of(&blocks[1]), Some("mine"));
    }

    /// An image alone, with no prose around it, produces no empty text blocks.
    #[test]
    fn a_bare_image_produces_no_empty_text_blocks() {
        let a = "a".repeat(64);
        let blocks = interleave_prompt_blocks(
            &format!("![image](http://relay/media/{a}.png)"),
            vec![(a, image("only"))],
        );
        assert_eq!(blocks.len(), 1);
        assert_eq!(data_of(&blocks[0]), Some("only"));
    }

    /// The read token is a signed kind-24242 event scoped to the relay.
    #[test]
    fn get_auth_is_a_scoped_blossom_token() {
        let fetcher = MediaFetcher::new("wss://hive.example", nostr::Keys::generate(), None)
            .expect("fetcher");
        let header = fetcher.sign_get_auth().expect("sign");
        let encoded = header.strip_prefix("Nostr ").expect("Nostr scheme");
        let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded)
            .expect("base64");
        let value: serde_json::Value =
            serde_json::from_slice(&raw).expect("the token is a signed event");
        assert_eq!(value["kind"], 24242);
        let tags = value["tags"].as_array().expect("tags");
        assert!(tags.iter().any(|tag| tag[0] == "t" && tag[1] == "get"));
        assert!(tags
            .iter()
            .any(|tag| tag[0] == "server" && tag[1] == "hive.example"));
    }
}
