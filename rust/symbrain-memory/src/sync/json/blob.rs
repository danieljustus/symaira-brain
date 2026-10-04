//! Byte-slice base64/array/null assignment with independent backing storage.

use super::super::{RelayBlob, SyncError};
use super::{
    raw::Raw,
    strings::field,
    typed::{Decode, Slice, object},
};
use base64::{
    Engine, alphabet,
    engine::{GeneralPurpose, GeneralPurposeConfig},
};
#[derive(Clone, Default)]
pub(super) struct Blob {
    pub value: RelayBlob,
    bytes: Slice<u8>,
}

pub(super) fn assign(raw: Raw<'_>, state: &mut Blob, decode: &mut Decode) -> Result<(), SyncError> {
    let Some(members) = object(raw, decode, "db.RelayBlob", "RelayResponse.blobs")? else {
        return Ok(());
    };
    for (name, raw) in members {
        match field(&name).as_str() {
            "id" => decode.string(raw, &mut state.value.id, "RelayBlob.blobs.id")?,
            "updated_at" => decode.clock(raw, &mut state.value.updated_at)?,
            "blob" => {
                if raw.kind() == "string" {
                    let text = super::strings::unquote(raw.0)?;
                    let clean: Vec<_> = text
                        .bytes()
                        .filter(|c| !matches!(c, b'\r' | b'\n'))
                        .collect();
                    let engine = GeneralPurpose::new(
                        &alphabet::STANDARD,
                        GeneralPurposeConfig::new().with_decode_allow_trailing_bits(true),
                    );
                    let result = super::base64::check(text.as_bytes()).and_then(|()| {
                        engine.decode(&clean).map_err(|_| {
                            SyncError("native base64 decoder diverged from pinned Go source".into())
                        })
                    });
                    match result {
                        Ok(bytes) => {
                            state.bytes.len = bytes.len();
                            state.bytes.slots = bytes.clone();
                            state.bytes.nil = false;
                            state.value.blob = bytes;
                            state.value.blob_present = true;
                        }
                        Err(error) => {
                            decode.error.get_or_insert(error);
                        }
                    }
                } else {
                    state.bytes.assign(
                        raw,
                        decode,
                        "[]uint8",
                        "RelayBlob.blobs.blob",
                        |raw, value, decode| {
                            let mut integer = i64::from(*value);
                            decode.integer(raw, &mut integer, "uint8", "RelayBlob.blobs.blob");
                            if let Ok(byte) = u8::try_from(integer) {
                                *value = byte;
                            } else {
                                decode.fail(raw, "uint8", "RelayBlob.blobs.blob");
                            }
                            Ok(())
                        },
                    )?;
                    state.value.blob = state.bytes.visible().unwrap_or_default();
                    state.value.blob_present = !state.bytes.nil;
                }
            }
            _ => {}
        }
    }
    Ok(())
}
