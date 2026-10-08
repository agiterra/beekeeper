//! Uploading a host step's scrubbed logs to the relay's media store (spec
//! § 7 C6).
//!
//! Off unless the step's `capture.upload` says so: a log is readable by every
//! channel member once uploaded, and only this host's own `env_from_host`
//! values are scrubbed from it — a command can print any other secret it
//! likes. The bytes uploaded are exactly the scrubbed bytes; the result
//! carries their SHA-256 so a reader can check what was published.
//!
//! The transport mirrors `bee upload` (`crates/beekeeper-cli/src/client.rs`
//! `upload_blob_bytes`): a Blossom `PUT /upload` with a kind:24242 auth
//! event naming the hash and the relay's authority. The relay's `/upload`
//! route accepts non-image files; only the legacy `/media/upload` refuses
//! them, so no fallback is attempted here.
//!
//! Images ([`ArtifactUploader::upload_image`], the device surface's durable
//! snapshots) are decoded and re-encoded first: the relay's media validation
//! accepts only metadata-free PNG/JPEG (`beekeeper-media/src/validation.rs`),
//! and a re-encode is also what guarantees no EXIF/ancillary chunk carries
//! anything host-local. The published `x` is the hash of the re-encoded bytes.

use std::time::Duration;

use base64::Engine;
use beekeeper_core::host_step::HostStepArtifact;
use sha2::{Digest, Sha256};

/// How long one upload may take.
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(120);
/// How long the upload auth event is valid.
const UPLOAD_AUTH_EXPIRY_SECS: u64 = 600;
/// The MIME type a log is uploaded as.
const LOG_MIME: &str = "text/plain; charset=utf-8";

/// A relay media endpoint reachable with this provider's identity.
#[derive(Debug, Clone)]
pub struct ArtifactUploader {
    http: reqwest::Client,
    base: String,
    authority: String,
    keys: nostr::Keys,
    auth_tag_json: Option<String>,
}

impl ArtifactUploader {
    /// Build an uploader for `relay_url` (`ws://`/`wss://` as configured;
    /// media is served over the matching HTTP scheme), or `None` when the URL
    /// has no authority.
    pub fn new(relay_url: &str, keys: nostr::Keys, auth_tag: Option<&nostr::Tag>) -> Option<Self> {
        let authority = beekeeper_core::tenant::relay_url_authority(relay_url);
        if authority.is_empty() {
            return None;
        }
        let scheme = if relay_url.starts_with("ws://") || relay_url.starts_with("http://") {
            "http"
        } else {
            "https"
        };
        let auth_tag_json = auth_tag.and_then(|tag| serde_json::to_string(tag).ok());
        Some(Self {
            http: reqwest::Client::builder()
                .timeout(UPLOAD_TIMEOUT)
                .build()
                .ok()?,
            base: format!("{scheme}://{authority}"),
            authority,
            keys,
            auth_tag_json,
        })
    }

    /// Upload `bytes` as the log named `name`, returning where the relay
    /// serves it. The caller scrubs first; this uploads exactly what it gets.
    pub async fn upload_log(&self, name: &str, bytes: Vec<u8>) -> Result<HostStepArtifact, String> {
        let sha256 = hex::encode(Sha256::digest(&bytes));
        let size = bytes.len() as u64;
        let url = self.put_blob(name, bytes, LOG_MIME, &sha256).await?;
        Ok(HostStepArtifact {
            name: name.to_owned(),
            url,
            sha256,
            bytes: size,
        })
    }

    /// Upload an image prepared by [`prepare_image`], returning where the
    /// relay serves it.
    pub async fn upload_prepared(
        &self,
        name: &str,
        image: &PreparedImage,
    ) -> Result<UploadedImage, String> {
        let url = self
            .put_blob(name, image.bytes.clone(), image.mime, &image.sha256)
            .await?;
        Ok(UploadedImage {
            url,
            sha256: image.sha256.clone(),
            bytes: image.bytes.len() as u64,
            mime: image.mime,
            width: image.width,
            height: image.height,
        })
    }

    /// Re-encode `bytes` as `mime` (`image/png` or `image/jpeg`) and upload
    /// the result. See [`prepare_image`].
    pub async fn upload_image(
        &self,
        name: &str,
        bytes: &[u8],
        mime: &str,
    ) -> Result<UploadedImage, String> {
        let image = prepare_image(bytes, mime)?;
        self.upload_prepared(name, &image).await
    }

    async fn put_blob(
        &self,
        name: &str,
        bytes: Vec<u8>,
        mime: &str,
        sha256: &str,
    ) -> Result<String, String> {
        let auth = self.sign_upload_auth(sha256)?;
        let mut request = self
            .http
            .put(format!("{}/upload", self.base))
            .header("Authorization", auth)
            .header("Content-Type", mime)
            .header("X-SHA-256", sha256)
            .body(bytes);
        if let Some(json) = &self.auth_tag_json {
            request = request.header("x-auth-tag", json);
        }
        let response = request
            .send()
            .await
            .map_err(|error| format!("upload of {name} failed: {error}"))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(format!(
                "the relay refused the upload of {name}: {} {}",
                status.as_u16(),
                body.trim()
            ));
        }
        let descriptor: serde_json::Value = response.json().await.map_err(|error| {
            format!("the relay's upload answer for {name} is not JSON: {error}")
        })?;
        descriptor
            .get("url")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| format!("the relay's upload answer for {name} names no url"))
    }

    fn sign_upload_auth(&self, sha256: &str) -> Result<String, String> {
        use nostr::{EventBuilder, JsonUtil, Kind, Tag, Timestamp};
        let now = Timestamp::now().as_secs();
        let tags = vec![
            Tag::parse(["t", "upload"]).map_err(|error| error.to_string())?,
            Tag::parse(["x", sha256]).map_err(|error| error.to_string())?,
            Tag::parse(["expiration", &(now + UPLOAD_AUTH_EXPIRY_SECS).to_string()])
                .map_err(|error| error.to_string())?,
            Tag::parse(["server", self.authority.as_str()]).map_err(|error| error.to_string())?,
        ];
        let event = EventBuilder::new(Kind::from(24242), "Upload host step log")
            .tags(tags)
            .sign_with_keys(&self.keys)
            .map_err(|error| error.to_string())?;
        Ok(format!(
            "Nostr {}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(event.as_json().as_bytes())
        ))
    }
}

/// An image re-encoded for upload: metadata-free bytes and their facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedImage {
    /// The re-encoded bytes — exactly what is uploaded and hashed.
    pub bytes: Vec<u8>,
    /// Lowercase hex SHA-256 of [`Self::bytes`].
    pub sha256: String,
    /// `image/png` or `image/jpeg`.
    pub mime: &'static str,
    /// Pixel width.
    pub width: u32,
    /// Pixel height.
    pub height: u32,
}

/// Where an uploaded image is served, and what it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadedImage {
    /// The relay's Blossom URL.
    pub url: String,
    /// SHA-256 of the uploaded bytes.
    pub sha256: String,
    /// Uploaded size.
    pub bytes: u64,
    /// MIME type.
    pub mime: &'static str,
    /// Pixel width.
    pub width: u32,
    /// Pixel height.
    pub height: u32,
}

/// Decode any supported image and re-encode it as `mime` with no metadata.
/// Blocking (CPU): call it off the run loop.
pub fn prepare_image(bytes: &[u8], mime: &str) -> Result<PreparedImage, String> {
    use image::ImageEncoder;
    let decoded =
        image::load_from_memory(bytes).map_err(|error| format!("not a readable image: {error}"))?;
    let (width, height) = (decoded.width(), decoded.height());
    let mut out = Vec::new();
    let mime: &'static str = match mime {
        "image/png" => {
            let rgba = decoded.to_rgba8();
            image::codecs::png::PngEncoder::new(&mut out)
                .write_image(
                    rgba.as_raw(),
                    width,
                    height,
                    image::ExtendedColorType::Rgba8,
                )
                .map_err(|error| format!("png encode: {error}"))?;
            "image/png"
        }
        "image/jpeg" => {
            let rgb = decoded.to_rgb8();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85)
                .write_image(rgb.as_raw(), width, height, image::ExtendedColorType::Rgb8)
                .map_err(|error| format!("jpeg encode: {error}"))?;
            "image/jpeg"
        }
        other => return Err(format!("{other} is not an uploadable image type")),
    };
    Ok(PreparedImage {
        sha256: hex::encode(Sha256::digest(&out)),
        bytes: out,
        mime,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_with_text_chunk() -> Vec<u8> {
        let img =
            image::RgbImage::from_fn(12, 7, |x, y| image::Rgb([x as u8 * 20, y as u8 * 30, 9]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("encode");
        // Splice a tEXt chunk (with a host path) in after IHDR (8 + 25 bytes).
        let mut chunk = Vec::new();
        let data = b"Comment\0/Users/brian/secret.png";
        chunk.extend_from_slice(&(data.len() as u32).to_be_bytes());
        chunk.extend_from_slice(b"tEXt");
        chunk.extend_from_slice(data);
        chunk.extend_from_slice(&[0, 0, 0, 0]);
        let mut out = png[..33].to_vec();
        out.extend_from_slice(&chunk);
        out.extend_from_slice(&png[33..]);
        out
    }

    #[test]
    fn images_are_reencoded_without_metadata_and_hashed_after() {
        let source = png_with_text_chunk();
        assert!(source.windows(7).any(|w| w == b"/Users/"));
        let png = prepare_image(&source, "image/png").expect("png");
        assert_eq!((png.mime, png.width, png.height), ("image/png", 12, 7));
        assert!(!png.bytes.windows(4).any(|w| w == b"tEXt"));
        assert_eq!(png.sha256, hex::encode(Sha256::digest(&png.bytes)));
        let jpeg = prepare_image(&source, "image/jpeg").expect("jpeg");
        assert_eq!(&jpeg.bytes[..2], &[0xFF, 0xD8]);
        assert!(prepare_image(&source, "image/gif").is_err());
        assert!(prepare_image(b"not an image", "image/png").is_err());
        // Deterministic: the same pixels re-encode to the same bytes.
        assert_eq!(
            prepare_image(&png.bytes, "image/png")
                .expect("again")
                .sha256,
            png.sha256
        );
    }

    #[test]
    fn the_media_base_follows_the_relay_scheme_and_needs_an_authority() {
        let up = ArtifactUploader::new("wss://hive.example", nostr::Keys::generate(), None)
            .expect("uploader");
        assert_eq!(up.base, "https://hive.example");
        let up = ArtifactUploader::new("ws://127.0.0.1:3000", nostr::Keys::generate(), None)
            .expect("uploader");
        assert_eq!(up.base, "http://127.0.0.1:3000");
        assert!(ArtifactUploader::new("", nostr::Keys::generate(), None).is_none());
    }

    #[test]
    fn the_upload_auth_names_the_hash_and_the_server() {
        let up = ArtifactUploader::new("wss://hive.example", nostr::Keys::generate(), None)
            .expect("uploader");
        let header = up.sign_upload_auth(&"a".repeat(64)).expect("auth");
        let encoded = header.strip_prefix("Nostr ").expect("scheme");
        let json = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded)
            .expect("base64");
        let event: serde_json::Value = serde_json::from_slice(&json).expect("event json");
        assert_eq!(event["kind"], 24242);
        let tags = event["tags"].as_array().expect("tags");
        assert!(tags.iter().any(|t| t[0] == "t" && t[1] == "upload"));
        assert!(tags.iter().any(|t| t[0] == "x" && t[1] == "a".repeat(64)));
        assert!(tags
            .iter()
            .any(|t| t[0] == "server" && t[1] == "hive.example"));
    }
}
