//! Fetch a turn's image attachments and render them as ACP image blocks.
//!
//! The operator uploads to the relay's Blossom store and the 44220 command
//! carries only the blob's hash; this module is the other half — it derives the
//! URL from the relay *this provider* is connected to, reads the bytes back
//! under a signed `t=get` token, and bounds what reaches a model.
//!
//! Addressing by hash rather than URL is deliberate. A URL in the payload would
//! let whoever signed the command choose where this process makes an outbound
//! request; a hash cannot.

use base64::Engine;
use buzz_acp::acp::PromptBlock;
use buzz_core::coding_session_command::TurnAttachment;
use image::{codecs::png::PngEncoder, DynamicImage, ImageEncoder, ImageReader, Limits};
use sha2::{Digest, Sha256};
use std::io::Cursor;
use std::time::Duration;

/// Lifetime of a Blossom `t=get` read token. Matches the desktop client's
/// `MEDIA_GET_AUTH_EXPIRY_SECS` and `buzz-dev-mcp`'s `view_image`.
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
}

impl MediaFetcher {
    /// Build a fetcher for the relay this provider is configured against.
    ///
    /// Returns `None` when the relay URL cannot be understood, which simply
    /// means attachments are not deliverable on this host — never a panic on a
    /// path a turn depends on.
    pub fn new(relay_url: &str, keys: nostr::Keys) -> Option<Self> {
        let authority = buzz_core::tenant::relay_url_authority(relay_url);
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
                .build()
                .ok()?,
            base: format!("{scheme}://{authority}"),
            authority,
            keys,
        })
    }

    /// Fetch every attachment and render it as an ACP image block.
    ///
    /// Attachments that cannot be fetched or decoded are skipped with a warning
    /// rather than failing the turn: the operator's words are the turn, and
    /// losing them because one blob 404'd would be the worse outcome. The count
    /// actually delivered is returned so the caller can tell the truth about it.
    /// Concurrently, and in the operator's order. Concurrency is not a
    /// micro-optimisation here: this runs before the turn's opening transcript
    /// item is emitted, so four sequential fetches would each add their own
    /// timeout to the delay before anyone sees the turn start.
    pub async fn image_blocks(&self, attachments: &[TurnAttachment]) -> Vec<(String, PromptBlock)> {
        let mut tasks = tokio::task::JoinSet::new();
        for (index, attachment) in attachments.iter().enumerate() {
            let fetcher = self.clone();
            let attachment = attachment.clone();
            tasks.spawn(async move {
                let result = fetcher.image_block(&attachment).await;
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

    async fn image_block(&self, attachment: &TurnAttachment) -> Result<PromptBlock, String> {
        let url = format!(
            "{}/media/{}.{}",
            self.base,
            attachment.sha256,
            attachment.extension()
        );
        let authorization = self.sign_get_auth()?;
        let response = self
            .http
            .get(&url)
            .header(reqwest::header::AUTHORIZATION, authorization)
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

        let (mime, encoded) = downscale_to_png(&bytes)?;
        Ok(PromptBlock::Image {
            mime,
            data_base64: base64::engine::general_purpose::STANDARD.encode(encoded),
        })
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

/// Interleave fetched images into the prompt at the positions the operator
/// wrote them.
///
/// The turn text carries a markdown reference per image
/// (`![image](…/media/<sha>.<ext>)`), which is also what makes the picture
/// render in the transcript. Splitting on those references means the agent
/// reads *"when I do X I see this:"*, then the screenshot, then *"but I want
/// to see:"*, then the next one — the same order a person reading the
/// transcript sees. Appending every image after the prose instead loses which
/// sentence each one belongs to, which for a two-screenshot before/after turn
/// is the entire meaning.
///
/// Images whose reference is absent from the text are appended rather than
/// dropped: a lost marker must not silently cost the agent a picture.
pub(crate) fn interleave_prompt_blocks(
    text: &str,
    mut images: Vec<(String, PromptBlock)>,
) -> Vec<PromptBlock> {
    let mut blocks = Vec::new();
    let mut rest = text;

    while let Some((before, sha, after)) = split_at_image_reference(rest, &images) {
        push_text(&mut blocks, before);
        if let Some(index) = images.iter().position(|(candidate, _)| *candidate == sha) {
            blocks.push(images.remove(index).1);
        }
        rest = after;
    }
    push_text(&mut blocks, rest);

    // Anything the prose never referenced still reaches the agent, after the
    // words, which is where an unpositioned image belongs.
    blocks.extend(images.into_iter().map(|(_, block)| block));
    blocks
}

/// Push a text block unless it is only whitespace.
fn push_text(blocks: &mut Vec<PromptBlock>, text: &str) {
    let trimmed = text.trim();
    if !trimmed.is_empty() {
        blocks.push(PromptBlock::Text(trimmed.to_owned()));
    }
}

/// Find the next `![...](url)` whose url names one of `images`.
///
/// Hand-rolled rather than a regex dependency: the shape is fixed and the only
/// thing that matters is the 64-hex blob id inside the parentheses. A
/// reference to an image this turn did not carry is left in the prose, because
/// rewriting text the operator signed is not this function's business.
fn split_at_image_reference<'a>(
    text: &'a str,
    images: &[(String, PromptBlock)],
) -> Option<(&'a str, String, &'a str)> {
    let mut cursor = 0usize;
    while let Some(open) = text[cursor..].find("![") {
        let open = cursor + open;
        let Some(close) = text[open..].find("](").map(|offset| open + offset) else {
            break;
        };
        let url_start = close + 2;
        let Some(end) = text[url_start..].find(')').map(|offset| url_start + offset) else {
            break;
        };
        let url = &text[url_start..end];
        if let Some(sha) = images
            .iter()
            .map(|(sha, _)| sha)
            .find(|sha| url.contains(sha.as_str()))
        {
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
        assert!(MediaFetcher::new("", nostr::Keys::generate()).is_none());
    }

    /// The `ws`/`wss` the provider is configured with maps to the matching
    /// HTTP scheme on the same authority — media is not served over WebSocket.
    #[test]
    fn media_base_follows_the_relay_scheme() {
        let secure =
            MediaFetcher::new("wss://hive.example", nostr::Keys::generate()).expect("fetcher");
        assert_eq!(secure.base, "https://hive.example");
        let plain =
            MediaFetcher::new("ws://127.0.0.1:3000", nostr::Keys::generate()).expect("fetcher");
        assert_eq!(plain.base, "http://127.0.0.1:3000");
    }

    /// The whole read path against a real HTTP server: the URL is derived from
    /// the relay and the hash, the request carries a Blossom `t=get` token, and
    /// the answer becomes an ACP image block.
    #[tokio::test]
    async fn a_blob_is_fetched_by_hash_and_becomes_an_image_block() {
        use axum::{extract::State, routing::get, Router};
        use std::sync::{Arc, Mutex};

        /// Requests the stub server saw: path, and the Authorization header.
        type Seen = Arc<Mutex<Vec<(String, Option<String>)>>>;

        let png = png_bytes(32, 16);
        let sha = hex::encode(Sha256::digest(&png));
        // What the server was asked for, so the test can assert the path and
        // the auth header rather than only the happy-path bytes.
        let seen: Seen = Arc::new(Mutex::new(Vec::new()));

        async fn serve(
            State((body, seen)): State<(Arc<Vec<u8>>, Seen)>,
            request: axum::extract::Request,
        ) -> Vec<u8> {
            let authorization = request
                .headers()
                .get(axum::http::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            seen.lock()
                .expect("lock")
                .push((request.uri().path().to_owned(), authorization));
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

        let fetcher = MediaFetcher::new(&format!("ws://{address}"), nostr::Keys::generate())
            .expect("fetcher");
        let attachment = TurnAttachment {
            sha256: sha.clone(),
            mime: "image/png".into(),
            size: png.len() as u64,
            dim: Some("32x16".into()),
            filename: Some("shot.png".into()),
        };

        let blocks = fetcher
            .image_blocks(std::slice::from_ref(&attachment))
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

        // A blob whose bytes do not match the hash the command declared is
        // refused: showing the agent something other than what the operator
        // attached is worse than showing it nothing.
        let tampered = TurnAttachment {
            sha256: "0".repeat(64),
            ..attachment
        };
        assert!(
            fetcher.image_blocks(&[tampered]).await.is_empty(),
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

        let fetcher = MediaFetcher::new(&format!("ws://{address}"), nostr::Keys::generate())
            .expect("fetcher");
        let describe = |sha: &str, size: usize| TurnAttachment {
            sha256: sha.to_owned(),
            mime: "image/png".into(),
            size: size as u64,
            dim: None,
            filename: None,
        };

        let blocks = fetcher
            .image_blocks(&[
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
        let fetcher =
            MediaFetcher::new("wss://hive.example", nostr::Keys::generate()).expect("fetcher");
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
