//! The brush table: the ACR blob index, the brotli-compressed mask brush payload, its cursor and its record parser.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MaskBrushError {
    class: MaskBrushTableRefusal,
    detail: String,
}

impl MaskBrushError {
    pub(super) fn new(class: MaskBrushTableRefusal, detail: impl Into<String>) -> Self {
        Self { class, detail: detail.into() }
    }
}

// The companion is an object store, not one allocation. Its file gate follows
// the RAW reader's per-file ceiling; the tighter gates below bound everything
// this parser actually materialises.
const MAX_ACR_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub(super) const MAX_ACR_DIRECTORY_ENTRIES: usize = 4_096;
const MAX_MASK_BRUSH_BLOB_BYTES: usize = 16 * 1024 * 1024;
const MAX_MASK_BRUSH_UNCOMPRESSED_BYTES: usize = 16 * 1024 * 1024;
const MAX_MASK_BRUSH_RECORDS: usize = 256;
const MAX_MASK_BRUSH_D_COUNT: usize = 65_536;
const MAX_MASK_BRUSH_TOKENS: usize = 65_536;

#[derive(Clone)]
struct AcrEntry {
    key: [u8; 16],
    len: u64,
    offset: u64,
}

#[derive(Clone)]
struct AcrIndex {
    path: std::path::PathBuf,
    entries: Vec<AcrEntry>,
}

pub(super) struct MaskBrushReader<'a, 'sink> {
    acr_path: Option<std::path::PathBuf>,
    diag: Option<&'a crate::diag::Diag<'sink>>,
    index: Option<Result<AcrIndex, MaskBrushError>>,
    tables: std::collections::HashMap<usize, Result<Vec<BrushStroke>, MaskBrushError>>,
    reported: std::collections::HashSet<usize>,
}

impl<'a, 'sink> MaskBrushReader<'a, 'sink> {
    pub(super) fn new(photo: Option<&std::path::Path>, diag: Option<&'a crate::diag::Diag<'sink>>) -> Self {
        Self {
            acr_path: photo.map(|p| p.with_extension("acr")),
            diag,
            index: None,
            tables: std::collections::HashMap::new(),
            reported: std::collections::HashSet::new(),
        }
    }

    pub(super) fn report(&mut self, owner_at: usize, token: &str, error: &MaskBrushError) {
        if self.reported.insert(owner_at)
            && let Some(diag) = self.diag
        {
            diag.warn(format!(
                "{}: MaskBrushTable {} refused ({})",
                error.class.name(),
                token,
                error.detail
            ));
        }
    }

    pub(super) fn table(
        &mut self,
        owner_at: usize,
        token: &str,
        expected: usize,
    ) -> Result<Vec<BrushStroke>, MaskBrushError> {
        if let Some(cached) = self.tables.get(&owner_at) {
            return cached.clone();
        }
        let result = self.read_table(token, expected);
        if let Err(error) = &result {
            self.report(owner_at, token, error);
        }
        self.tables.insert(owner_at, result.clone());
        result
    }

    fn read_table(
        &mut self,
        token: &str,
        expected: usize,
    ) -> Result<Vec<BrushStroke>, MaskBrushError> {
        let key = mask_brush_key(token)?;
        if expected > MAX_MASK_BRUSH_UNCOMPRESSED_BYTES {
            return Err(MaskBrushError::new(
                MaskBrushTableRefusal::LengthMismatch,
                format!(
                    "advertised output {expected} exceeds the {MAX_MASK_BRUSH_UNCOMPRESSED_BYTES}-byte limit"
                ),
            ));
        }
        if self.index.is_none() {
            self.index = Some(match &self.acr_path {
                Some(path) => load_acr_index(path),
                None => Err(MaskBrushError::new(
                    MaskBrushTableRefusal::MaskBrushTableUnavailable,
                    "no photo path was available for sibling .acr discovery",
                )),
            });
        }
        let index = match self.index.as_ref().expect("index was initialized") {
            Ok(index) => index,
            Err(error) => return Err(error.clone()),
        };
        let mut matches = index.entries.iter().filter(|entry| entry.key == key);
        let Some(entry) = matches.next() else {
            return Err(MaskBrushError::new(
                MaskBrushTableRefusal::ReferenceMismatch,
                "directory contains no matching key",
            ));
        };
        if matches.next().is_some() {
            return Err(MaskBrushError::new(
                MaskBrushTableRefusal::ReferenceMismatch,
                "directory contains a duplicate matching key",
            ));
        }
        let blob_len = usize::try_from(entry.len).map_err(|_| {
            MaskBrushError::new(MaskBrushTableRefusal::Corrupt, "blob length does not fit memory")
        })?;
        if blob_len > MAX_MASK_BRUSH_BLOB_BYTES {
            return Err(MaskBrushError::new(
                MaskBrushTableRefusal::Corrupt,
                format!("blob exceeds the {MAX_MASK_BRUSH_BLOB_BYTES}-byte limit"),
            ));
        }
        let mut blob = Vec::new();
        blob.try_reserve_exact(blob_len).map_err(|_| {
            MaskBrushError::new(MaskBrushTableRefusal::Corrupt, "blob allocation refused")
        })?;
        blob.resize(blob_len, 0);
        let mut file = std::fs::File::open(&index.path).map_err(|error| {
            MaskBrushError::new(
                MaskBrushTableRefusal::MaskBrushTableUnavailable,
                format!("cannot reopen {}: {error}", index.path.display()),
            )
        })?;
        use std::io::{Read as _, Seek as _};
        file.seek(std::io::SeekFrom::Start(entry.offset)).map_err(|error| {
            MaskBrushError::new(
                MaskBrushTableRefusal::ContainerInvalid,
                format!("cannot seek to object: {error}"),
            )
        })?;
        file.read_exact(&mut blob).map_err(|error| {
            MaskBrushError::new(
                MaskBrushTableRefusal::ContainerInvalid,
                format!("object range became unreadable: {error}"),
            )
        })?;
        if md5::compute(&blob).0 != key {
            return Err(MaskBrushError::new(
                MaskBrushTableRefusal::DigestMismatch,
                "MD5(blob) does not equal the XMP/directory key",
            ));
        }
        if blob.len() < 16 {
            return Err(MaskBrushError::new(
                MaskBrushTableRefusal::EncodingUnsupported,
                "object is shorter than the 16-byte envelope",
            ));
        }
        let envelope = [
            le_u32_at(&blob, 0),
            le_u32_at(&blob, 4),
            le_u32_at(&blob, 8),
            le_u32_at(&blob, 12),
        ];
        let stream_len = blob.len() - 16;
        if envelope != [4, 1, 64_000, stream_len as u32] {
            return Err(MaskBrushError::new(
                MaskBrushTableRefusal::EncodingUnsupported,
                format!("unsupported object envelope {envelope:?}"),
            ));
        }
        let payload = decode_mask_brush_brotli(&blob[16..], expected)?;
        parse_mask_brush_payload(&payload)
    }
}

fn load_acr_index(path: &std::path::Path) -> Result<AcrIndex, MaskBrushError> {
    use std::io::Read as _;
    let mut file = std::fs::File::open(path).map_err(|error| {
        MaskBrushError::new(
            MaskBrushTableRefusal::MaskBrushTableUnavailable,
            format!("cannot read sibling {}: {error}", path.display()),
        )
    })?;
    let file_len = file.metadata().map_err(|error| {
        MaskBrushError::new(
            MaskBrushTableRefusal::MaskBrushTableUnavailable,
            format!("cannot inspect sibling {}: {error}", path.display()),
        )
    })?.len();
    if file_len > MAX_ACR_BYTES {
        return Err(MaskBrushError::new(
            MaskBrushTableRefusal::ContainerInvalid,
            format!("ACR file exceeds the {MAX_ACR_BYTES}-byte limit"),
        ));
    }
    let mut header = [0u8; 20];
    file.read_exact(&mut header).map_err(|error| {
        MaskBrushError::new(
            MaskBrushTableRefusal::ContainerInvalid,
            format!("truncated ACR header: {error}"),
        )
    })?;
    if &header[0..4] != b"ACR\0"
        || le_u32_at(&header, 4) != 1
        || &header[8..12] != b"ARW\0"
        || le_u32_at(&header, 16) != 0
    {
        return Err(MaskBrushError::new(
            MaskBrushTableRefusal::ContainerInvalid,
            "header is not the established ACR/1/ARW/reserved-zero shape",
        ));
    }
    let count = le_u32_at(&header, 12) as usize;
    if count > MAX_ACR_DIRECTORY_ENTRIES {
        return Err(MaskBrushError::new(
            MaskBrushTableRefusal::ContainerInvalid,
            format!("directory count exceeds the {MAX_ACR_DIRECTORY_ENTRIES}-entry limit"),
        ));
    }
    let directory_bytes = count.checked_mul(32).ok_or_else(|| {
        MaskBrushError::new(MaskBrushTableRefusal::ContainerInvalid, "directory size overflow")
    })?;
    let directory_end = 20u64.checked_add(directory_bytes as u64).ok_or_else(|| {
        MaskBrushError::new(MaskBrushTableRefusal::ContainerInvalid, "directory end overflow")
    })?;
    if directory_end > file_len {
        return Err(MaskBrushError::new(
            MaskBrushTableRefusal::ContainerInvalid,
            "directory extends past end of file",
        ));
    }
    let mut raw = Vec::new();
    raw.try_reserve_exact(directory_bytes).map_err(|_| {
        MaskBrushError::new(
            MaskBrushTableRefusal::ContainerInvalid,
            "directory allocation refused",
        )
    })?;
    raw.resize(directory_bytes, 0);
    file.read_exact(&mut raw).map_err(|error| {
        MaskBrushError::new(
            MaskBrushTableRefusal::ContainerInvalid,
            format!("truncated ACR directory: {error}"),
        )
    })?;
    let mut entries = Vec::new();
    entries.try_reserve_exact(count).map_err(|_| {
        MaskBrushError::new(
            MaskBrushTableRefusal::ContainerInvalid,
            "entry allocation refused",
        )
    })?;
    for chunk in raw.chunks_exact(32) {
        let mut key = [0u8; 16];
        key.copy_from_slice(&chunk[..16]);
        let len = le_u64_at(chunk, 16);
        let offset = le_u64_at(chunk, 24);
        let end = offset.checked_add(len).ok_or_else(|| {
            MaskBrushError::new(MaskBrushTableRefusal::ContainerInvalid, "object range overflow")
        })?;
        if len == 0 || offset < directory_end || end > file_len {
            return Err(MaskBrushError::new(
                MaskBrushTableRefusal::ContainerInvalid,
                "object range is empty, overlaps the directory, or is out of bounds",
            ));
        }
        entries.push(AcrEntry { key, len, offset });
    }
    let mut ranges = Vec::new();
    ranges.try_reserve_exact(entries.len()).map_err(|_| {
        MaskBrushError::new(
            MaskBrushTableRefusal::ContainerInvalid,
            "range allocation refused",
        )
    })?;
    ranges.extend(entries.iter().map(|entry| (entry.offset, entry.offset + entry.len)));
    ranges.sort_unstable();
    if ranges.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return Err(MaskBrushError::new(
            MaskBrushTableRefusal::ContainerInvalid,
            "directory object ranges overlap",
        ));
    }
    let mut cursor = directory_end;
    for &(start, end) in &ranges {
        let padding = (4 - cursor % 4) % 4;
        if start != cursor + padding {
            return Err(MaskBrushError::new(
                MaskBrushTableRefusal::ContainerInvalid,
                "object gap is not the established four-byte alignment padding",
            ));
        }
        validate_acr_padding(&mut file, cursor, padding)?;
        cursor = end;
    }
    let trailing = (4 - cursor % 4) % 4;
    if file_len != cursor + trailing {
        return Err(MaskBrushError::new(
            MaskBrushTableRefusal::ContainerInvalid,
            "trailing gap is not the established four-byte alignment padding",
        ));
    }
    validate_acr_padding(&mut file, cursor, trailing)?;
    Ok(AcrIndex { path: path.to_path_buf(), entries })
}

fn validate_acr_padding(
    file: &mut std::fs::File,
    offset: u64,
    len: u64,
) -> Result<(), MaskBrushError> {
    use std::io::{Read as _, Seek as _};
    let mut padding = [0u8; 3];
    let len = usize::try_from(len).expect("four-byte alignment padding is at most three bytes");
    file.seek(std::io::SeekFrom::Start(offset)).map_err(|error| {
        MaskBrushError::new(
            MaskBrushTableRefusal::ContainerInvalid,
            format!("cannot seek to alignment padding: {error}"),
        )
    })?;
    file.read_exact(&mut padding[..len]).map_err(|error| {
        MaskBrushError::new(
            MaskBrushTableRefusal::ContainerInvalid,
            format!("cannot read alignment padding: {error}"),
        )
    })?;
    if padding[..len].iter().any(|byte| *byte != 0) {
        return Err(MaskBrushError::new(
            MaskBrushTableRefusal::ContainerInvalid,
            "alignment padding is not zero-filled",
        ));
    }
    Ok(())
}

fn mask_brush_key(token: &str) -> Result<[u8; 16], MaskBrushError> {
    if token.len() != 32 || !token.is_ascii() {
        return Err(MaskBrushError::new(
            MaskBrushTableRefusal::ReferenceMismatch,
            "reference is not exactly 32 ASCII hex bytes",
        ));
    }
    let mut key = [0u8; 16];
    for (i, pair) in token.as_bytes().chunks_exact(2).enumerate() {
        let hex = |b: u8| match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        };
        let (Some(hi), Some(lo)) = (hex(pair[0]), hex(pair[1])) else {
            return Err(MaskBrushError::new(
                MaskBrushTableRefusal::ReferenceMismatch,
                "reference contains a non-hex byte",
            ));
        };
        key[i] = (hi << 4) | lo;
    }
    Ok(key)
}

fn decode_mask_brush_brotli(
    stream: &[u8],
    expected: usize,
) -> Result<Vec<u8>, MaskBrushError> {
    use brotli_decompressor::{
        BrotliDecompressStream, BrotliResult, BrotliState, StandardAlloc,
    };
    let mut state = BrotliState::new(
        StandardAlloc::default(),
        StandardAlloc::default(),
        StandardAlloc::default(),
    );
    let mut available_in = stream.len();
    let mut input_offset = 0usize;
    let mut buffer = [0u8; 4_096];
    let mut available_out = buffer.len();
    let mut output_offset = 0usize;
    let mut total_out = 0usize;
    let mut output = Vec::new();
    output.try_reserve_exact(expected).map_err(|_| {
        MaskBrushError::new(MaskBrushTableRefusal::Corrupt, "output allocation refused")
    })?;
    loop {
        let result = BrotliDecompressStream(
            &mut available_in,
            &mut input_offset,
            stream,
            &mut available_out,
            &mut output_offset,
            &mut buffer,
            &mut total_out,
            &mut state,
        );
        if output.len().saturating_add(output_offset) > expected
            || output.len().saturating_add(output_offset)
                > MAX_MASK_BRUSH_UNCOMPRESSED_BYTES
        {
            return Err(MaskBrushError::new(
                MaskBrushTableRefusal::LengthMismatch,
                "Brotli output exceeded the advertised or implementation limit",
            ));
        }
        output.extend_from_slice(&buffer[..output_offset]);
        output_offset = 0;
        available_out = buffer.len();
        match result {
            BrotliResult::ResultSuccess => {
                if available_in != 0 || input_offset != stream.len() {
                    return Err(MaskBrushError::new(
                        MaskBrushTableRefusal::Corrupt,
                        "Brotli stream has trailing input",
                    ));
                }
                break;
            }
            BrotliResult::NeedsMoreInput if available_in == 0 => {
                return Err(MaskBrushError::new(
                    MaskBrushTableRefusal::Corrupt,
                    "Brotli stream is truncated",
                ));
            }
            BrotliResult::ResultFailure => {
                return Err(MaskBrushError::new(
                    MaskBrushTableRefusal::Corrupt,
                    "Brotli decoder rejected the stream",
                ));
            }
            BrotliResult::NeedsMoreInput | BrotliResult::NeedsMoreOutput => {}
        }
    }
    if output.len() != expected {
        return Err(MaskBrushError::new(
            MaskBrushTableRefusal::LengthMismatch,
            format!("decoded {} bytes, XMP advertises {expected}", output.len()),
        ));
    }
    Ok(output)
}

struct MaskBrushCursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> MaskBrushCursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], MaskBrushError> {
        let end = self.at.checked_add(len).ok_or_else(|| {
            MaskBrushError::new(MaskBrushTableRefusal::PayloadInvalid, "payload offset overflow")
        })?;
        let out = self.bytes.get(self.at..end).ok_or_else(|| {
            MaskBrushError::new(MaskBrushTableRefusal::PayloadInvalid, "truncated payload field")
        })?;
        self.at = end;
        Ok(out)
    }

    fn u16(&mut self) -> Result<u16, MaskBrushError> {
        let bytes: [u8; 2] = self.take(2)?.try_into().expect("two-byte slice");
        Ok(u16::from_le_bytes(bytes))
    }

    fn u32(&mut self) -> Result<u32, MaskBrushError> {
        let bytes: [u8; 4] = self.take(4)?.try_into().expect("four-byte slice");
        Ok(u32::from_le_bytes(bytes))
    }
}

fn parse_mask_brush_payload(bytes: &[u8]) -> Result<Vec<BrushStroke>, MaskBrushError> {
    let mut cursor = MaskBrushCursor::new(bytes);
    if cursor.u32()? != 1 {
        return Err(MaskBrushError::new(
            MaskBrushTableRefusal::PayloadUnsupported,
            "table word is not 1",
        ));
    }
    let record_count = cursor.u32()? as usize;
    if record_count > MAX_MASK_BRUSH_RECORDS {
        return Err(MaskBrushError::new(
            MaskBrushTableRefusal::PayloadInvalid,
            format!("record count exceeds the {MAX_MASK_BRUSH_RECORDS}-record limit"),
        ));
    }
    if record_count
        .checked_mul(70)
        .and_then(|n| n.checked_add(8))
        .is_none_or(|minimum| minimum > bytes.len())
    {
        return Err(MaskBrushError::new(
            MaskBrushTableRefusal::PayloadInvalid,
            "record count cannot fit in the payload",
        ));
    }
    let mut records = Vec::new();
    records.try_reserve_exact(record_count).map_err(|_| {
        MaskBrushError::new(MaskBrushTableRefusal::PayloadInvalid, "record allocation refused")
    })?;
    let mut table_tokens = 0usize;
    for _ in 0..record_count {
        let what = cursor.u32()?;
        let active = cursor.u32()?;
        let blend = cursor.u32()?;
        let inverted = cursor.u16()?;
        let id_len = cursor.u32()? as usize;
        if what != 0 || active > 1 || blend != 0 || inverted != 0 || id_len != 32 {
            return Err(MaskBrushError::new(
                MaskBrushTableRefusal::PayloadUnsupported,
                format!(
                    "unsupported record fields What={what}, active={active}, blend={blend}, inverted={inverted}, id_len={id_len}"
                ),
            ));
        }
        let id = cursor.take(id_len)?;
        if !id.is_ascii() {
            return Err(MaskBrushError::new(
                MaskBrushTableRefusal::PayloadUnsupported,
                "MaskSyncID is not ASCII",
            ));
        }
        let sync_id = std::str::from_utf8(id)
            .expect("ASCII is UTF-8")
            .to_string();
        let value = cursor.u32()?;
        let radius = cursor.u32()?;
        let flow = cursor.u32()?;
        let center_weight = cursor.u32()?;
        let d_count = cursor.u32()? as usize;
        if d_count > MAX_MASK_BRUSH_D_COUNT {
            return Err(MaskBrushError::new(
                MaskBrushTableRefusal::PayloadInvalid,
                format!("d-count exceeds the {MAX_MASK_BRUSH_D_COUNT}-dab limit"),
            ));
        }
        let mut d_seen = 0usize;
        let mut dabs = String::new();
        while d_seen < d_count {
            if table_tokens >= MAX_MASK_BRUSH_TOKENS {
                return Err(MaskBrushError::new(
                    MaskBrushTableRefusal::PayloadInvalid,
                    format!("token count exceeds the {MAX_MASK_BRUSH_TOKENS}-token limit"),
                ));
            }
            let opcode = cursor.take(1)?[0];
            let token = match opcode {
                0x01 => format!("r {}", fixed_decimal(cursor.u32()?, 6)),
                0x02 => format!("f {}", fixed_decimal(cursor.u32()?, 4)),
                0x06 => {
                    d_seen += 1;
                    format!(
                        "d {} {}",
                        fixed_decimal(cursor.u32()?, 6),
                        fixed_decimal(cursor.u32()?, 6)
                    )
                }
                _ => {
                    return Err(MaskBrushError::new(
                        MaskBrushTableRefusal::PayloadUnsupported,
                        format!("unsupported opcode 0x{opcode:02X}"),
                    ));
                }
            };
            if !dabs.is_empty() {
                dabs.push('\n');
            }
            dabs.push_str(&token);
            table_tokens += 1;
        }
        records.push(BrushStroke {
            // A valid inactive record remains in table order but contributes no
            // density; the original bytes remain authoritative on write-back.
            value: if active == 0 { 0.0 } else { value as f32 / 1_000_000.0 },
            radius: radius as f32 / 1_000_000.0,
            flow: flow as f32 / 1_000_000.0,
            center_weight: center_weight as f32 / 1_000_000.0,
            sync_id,
            dabs,
        });
    }
    if cursor.at != bytes.len() {
        return Err(MaskBrushError::new(
            MaskBrushTableRefusal::PayloadInvalid,
            format!("{} trailing payload byte(s)", bytes.len() - cursor.at),
        ));
    }
    Ok(records)
}

fn fixed_decimal(value: u32, places: usize) -> String {
    let scale = 10u64.pow(places as u32);
    let value = u64::from(value);
    let whole = value / scale;
    let fraction = value % scale;
    if fraction == 0 {
        return whole.to_string();
    }
    let mut out = format!("{whole}.{fraction:0places$}");
    while out.ends_with('0') {
        out.pop();
    }
    out
}

fn le_u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("validated fixed-width field"))
}

pub(super) fn le_u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().expect("validated fixed-width field"))
}
