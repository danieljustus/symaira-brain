//! Bounded response decoding and charset normalization.
use crate::{BodyTooLarge, FetchError};
use brotli::Decompressor;
use encoding_rs::Encoding;
use flate2::read::MultiGzDecoder;
use futures_util::StreamExt;
use reqwest::Response as HttpResponse;
use std::io::{Cursor, Read};

pub(super) struct ReadResponse {
    pub(super) final_url: String,
    pub(super) headers: std::collections::BTreeMap<String, Vec<String>>,
    pub(super) body: Vec<u8>,
    pub(super) protocol: String,
    pub(super) content_type: Option<String>,
}

pub(super) async fn read_response(
    response: HttpResponse,
    requested_url: &str,
    max_compressed: usize,
    max_body: usize,
    implicit_gzip: bool,
    body_permitted: bool,
) -> Result<ReadResponse, FetchError> {
    let final_url = response.url().to_string();
    let protocol = match response.version() {
        reqwest::Version::HTTP_2 => "HTTP/2.0",
        reqwest::Version::HTTP_3 => "HTTP/3.0",
        _ => "HTTP/1.1",
    }
    .to_owned();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let encoding = response
        .headers()
        .get("content-encoding")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let mut headers = std::collections::BTreeMap::new();
    for (name, values) in response.headers().iter() {
        headers
            .entry(name.to_string())
            .or_insert_with(Vec::new)
            .push(values.to_str().unwrap_or("").to_owned());
    }
    if response
        .content_length()
        .is_some_and(|length| length > max_compressed as u64)
    {
        return Err(FetchError::BodyTooLarge(BodyTooLarge {
            url: requested_url.to_owned(),
            limit: max_compressed,
            compressed: true,
        }));
    }
    let mut stream = response.bytes_stream();
    let mut compressed = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| FetchError::BodyRead(error.to_string()))?;
        if compressed.len().saturating_add(chunk.len()) > max_compressed {
            return Err(FetchError::BodyTooLarge(BodyTooLarge {
                url: requested_url.to_owned(),
                limit: max_compressed,
                compressed: true,
            }));
        }
        compressed.extend_from_slice(&chunk);
    }
    let decoded = if body_permitted {
        // Go only decompresses gzip when the transport advertised it implicitly.
        // Explicit gzip / Range responses retain their wire body and headers.
        let decoding = if encoding == "gzip" && !implicit_gzip {
            ""
        } else {
            &encoding
        };
        decode_content(&compressed, decoding, max_body, requested_url)?
    } else {
        compressed
    };
    // net/http strips transport metadata only for its implicit gzip decoding.
    if implicit_gzip && encoding == "gzip" {
        headers.remove("content-encoding");
        headers.remove("content-length");
    }
    let body = normalize_charset(decoded, content_type.as_deref());
    Ok(ReadResponse {
        final_url,
        headers,
        body,
        protocol,
        content_type,
    })
}

fn decode_content(
    input: &[u8],
    encoding: &str,
    max: usize,
    url: &str,
) -> Result<Vec<u8>, FetchError> {
    let mut decoded = input.to_vec();
    let encodings: Vec<_> = encoding
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect();
    for encoding in encodings.iter().rev() {
        let mut reader: Box<dyn Read> = match encoding.to_ascii_lowercase().as_str() {
            "identity" => Box::new(Cursor::new(decoded)),
            "gzip" | "x-gzip" => Box::new(MultiGzDecoder::new(Cursor::new(decoded))),
            "br" => Box::new(Decompressor::new(Cursor::new(decoded), 4096)),
            "zstd" => Box::new(
                ruzstd::decoding::StreamingDecoder::new_with_max_window_size(
                    Cursor::new(decoded),
                    max.max(1) as u64,
                )
                .map_err(|error| FetchError::Decode(error.to_string()))?,
            ),
            other => return Err(FetchError::UnsupportedEncoding(other.to_owned())),
        };
        decoded = read_limited(&mut reader, max).map_err(|error| match error {
            LimitError::TooLarge => FetchError::BodyTooLarge(BodyTooLarge {
                url: url.to_owned(),
                limit: max,
                compressed: false,
            }),
            LimitError::Read(error) => FetchError::Decode(error),
        })?;
    }
    if encodings.is_empty() && decoded.len() > max {
        return Err(FetchError::BodyTooLarge(BodyTooLarge {
            url: url.to_owned(),
            limit: max,
            compressed: false,
        }));
    }
    Ok(decoded)
}

enum LimitError {
    TooLarge,
    Read(String),
}

fn read_limited(reader: &mut dyn Read, max: usize) -> Result<Vec<u8>, LimitError> {
    let mut output = Vec::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| LimitError::Read(error.to_string()))?;
        if read == 0 {
            break;
        }
        if output.len().saturating_add(read) > max {
            return Err(LimitError::TooLarge);
        }
        output.extend_from_slice(&buffer[..read]);
    }
    Ok(output)
}

fn normalize_charset(body: Vec<u8>, content_type: Option<&str>) -> Vec<u8> {
    let label = content_type
        .and_then(charset_from_content_type)
        .map(str::to_owned)
        .or_else(|| sniff_meta_charset(&body));
    let Some(label) = label else {
        return String::from_utf8_lossy(&body).into_owned().into_bytes();
    };
    let Some(encoding) = Encoding::for_label(label.as_bytes()) else {
        return String::from_utf8_lossy(&body).into_owned().into_bytes();
    };
    let (text, _, _) = encoding.decode(&body);
    text.into_owned().into_bytes()
}

fn charset_from_content_type(content_type: &str) -> Option<&str> {
    content_type.split(';').find_map(|part| {
        let (name, value) = part.split_once('=')?;
        name.trim()
            .eq_ignore_ascii_case("charset")
            .then_some(value.trim().trim_matches(['"', '\'']))
    })
}

fn sniff_meta_charset(body: &[u8]) -> Option<String> {
    let prefix = String::from_utf8_lossy(&body[..body.len().min(8192)]);
    let lower = prefix.to_ascii_lowercase();
    let start = lower.find("charset")? + "charset".len();
    let remainder = prefix.get(start..)?.trim_start();
    let remainder = remainder.strip_prefix('=')?.trim_start();
    let remainder = remainder.strip_prefix(['"', '\'']).unwrap_or(remainder);
    let end = remainder
        .find(|character: char| {
            character.is_ascii_whitespace() || matches!(character, '"' | '\'' | '>' | ';')
        })
        .unwrap_or(remainder.len());
    let label = remainder.get(..end)?.trim();
    (!label.is_empty()).then_some(label.to_owned())
}
