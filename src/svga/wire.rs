//! Minimal protobuf wire walker. Fields are never decoded into typed values:
//! every field keeps its original bytes so untouched data is copied verbatim.
use super::Refusal;

pub(super) const LENGTH_DELIMITED: u8 = 2;
const MAX_FIELD_NUMBER: u64 = (1 << 29) - 1;
const MAX_VARINT_BYTES: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Field<'a> {
    pub number: u32,
    pub wire_type: u8,
    /// Tag, length prefix and payload exactly as stored.
    pub raw: &'a [u8],
    pub payload: &'a [u8],
}

fn varint(bytes: &[u8], at: usize) -> Result<(u64, usize), Refusal> {
    let mut value = 0u64;
    for index in 0..MAX_VARINT_BYTES {
        let byte = *bytes
            .get(at.checked_add(index).ok_or(TRUNCATED_VARINT)?)
            .ok_or(TRUNCATED_VARINT)?;
        let bits = u64::from(byte & 0x7f);
        // The tenth byte only has room for the top bit of a u64.
        if index == MAX_VARINT_BYTES - 1 && bits > 1 {
            return Err(Refusal::Malformed("svga_varint_overflow"));
        }
        value |= bits << (7 * index);
        if byte & 0x80 == 0 {
            return Ok((value, at + index + 1));
        }
    }
    Err(Refusal::Malformed("svga_varint_overflow"))
}

const TRUNCATED_VARINT: Refusal = Refusal::Malformed("svga_truncated_varint");
const TRUNCATED_FIELD: Refusal = Refusal::Malformed("svga_truncated_field");

/// Split one message into its fields without interpreting nested messages.
pub(super) fn walk(bytes: &[u8]) -> Result<Vec<Field<'_>>, Refusal> {
    let mut fields = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        let (tag, after_tag) = varint(bytes, offset)?;
        let number = tag >> 3;
        if number == 0 || number > MAX_FIELD_NUMBER {
            return Err(Refusal::Malformed("svga_invalid_field_number"));
        }
        let wire_type = (tag & 7) as u8;
        let (start, end) = match wire_type {
            0 => (after_tag, varint(bytes, after_tag)?.1),
            1 => (after_tag, after_tag.checked_add(8).ok_or(TRUNCATED_FIELD)?),
            5 => (after_tag, after_tag.checked_add(4).ok_or(TRUNCATED_FIELD)?),
            LENGTH_DELIMITED => {
                let (length, start) = varint(bytes, after_tag)?;
                let length = usize::try_from(length).map_err(|_| TRUNCATED_FIELD)?;
                (start, start.checked_add(length).ok_or(TRUNCATED_FIELD)?)
            }
            _ => return Err(Refusal::Malformed("svga_unsupported_wire_type")),
        };
        let raw = bytes.get(offset..end).ok_or(TRUNCATED_FIELD)?;
        let payload = bytes.get(start..end).ok_or(TRUNCATED_FIELD)?;
        fields.push(Field {
            number: number as u32,
            wire_type,
            raw,
            payload,
        });
        offset = end;
    }
    Ok(fields)
}

fn encode_varint(mut value: u64) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(MAX_VARINT_BYTES);
    while value >= 0x80 {
        bytes.push((value as u8) | 0x80);
        value >>= 7;
    }
    bytes.push(value as u8);
    bytes
}

/// A length-delimited field with a canonical tag and length prefix.
pub(super) fn length_delimited(number: u32, payload: &[u8]) -> Vec<u8> {
    let tag = (u64::from(number) << 3) | u64::from(LENGTH_DELIMITED);
    [
        encode_varint(tag),
        encode_varint(payload.len() as u64),
        payload.to_vec(),
    ]
    .concat()
}
