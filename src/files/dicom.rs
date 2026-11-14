use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagLevel {
    Patient,
    Study,
    Series,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DicomMetadata {
    pub patient: BTreeMap<String, String>,
    pub study: BTreeMap<String, String>,
    pub series: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct DicomPixelData {
    pub rows: u16,
    pub cols: u16,
    pub bits_allocated: u16,
    pub data: Vec<u8>,
}

/// Look up tag rule per `docs/dcm_tags_rules.md`.
pub fn get_tag_info(group: u16, element: u16) -> Option<(TagLevel, &'static str)> {
    match (group, element) {
        // Patient Level
        (0x0010, 0x0020) => Some((TagLevel::Patient, "PatientID")),
        (0x0010, 0x0010) => Some((TagLevel::Patient, "PatientName")),
        (0x0010, 0x0030) => Some((TagLevel::Patient, "PatientBirthDate")),
        (0x0010, 0x0040) => Some((TagLevel::Patient, "PatientSex")),
        (0x0010, 0x1010) => Some((TagLevel::Patient, "PatientAge")),
        (0x0010, 0x1020) => Some((TagLevel::Patient, "PatientSize")),
        (0x0010, 0x1030) => Some((TagLevel::Patient, "PatientWeight")),
        (0x0010, 0x2160) => Some((TagLevel::Patient, "EthnicGroup")),

        // Study Level
        (0x0020, 0x000D) => Some((TagLevel::Study, "StudyInstanceUID")),
        (0x0020, 0x0010) => Some((TagLevel::Study, "StudyID")),
        (0x0008, 0x0050) => Some((TagLevel::Study, "AccessionNumber")),
        (0x0008, 0x0020) => Some((TagLevel::Study, "StudyDate")),
        (0x0008, 0x0030) => Some((TagLevel::Study, "StudyTime")),
        (0x0008, 0x1030) => Some((TagLevel::Study, "StudyDescription")),
        (0x0008, 0x0090) => Some((TagLevel::Study, "ReferringPhysicianName")),
        (0x0008, 0x0080) => Some((TagLevel::Study, "InstitutionName")),
        (0x0008, 0x0061) => Some((TagLevel::Study, "ModalitiesInStudy")),

        // Series Level
        (0x0020, 0x000E) => Some((TagLevel::Series, "SeriesInstanceUID")),
        (0x0020, 0x0011) => Some((TagLevel::Series, "SeriesNumber")),
        (0x0008, 0x0060) => Some((TagLevel::Series, "Modality")),
        (0x0008, 0x103E) => Some((TagLevel::Series, "SeriesDescription")),
        (0x0008, 0x0021) => Some((TagLevel::Series, "SeriesDate")),
        (0x0008, 0x0031) => Some((TagLevel::Series, "SeriesTime")),
        (0x0018, 0x0015) => Some((TagLevel::Series, "BodyPartExamined")),
        (0x0018, 0x1030) => Some((TagLevel::Series, "ProtocolName")),
        (0x0018, 0x5100) => Some((TagLevel::Series, "PatientPosition")),
        (0x0020, 0x0052) => Some((TagLevel::Series, "FrameOfReferenceUID")),
        (0x0008, 0x0070) => Some((TagLevel::Series, "Manufacturer")),
        (0x0008, 0x1090) => Some((TagLevel::Series, "ManufacturerModelName")),

        _ => None,
    }
}

/// Look up tag specification (group, element, VR) by standard keyword.
pub fn get_tag_spec(keyword: &str) -> Option<(u16, u16, &'static str)> {
    match keyword {
        // Patient Level
        "PatientID" => Some((0x0010, 0x0020, "LO")),
        "PatientName" => Some((0x0010, 0x0010, "PN")),
        "PatientBirthDate" => Some((0x0010, 0x0030, "DA")),
        "PatientSex" => Some((0x0010, 0x0040, "CS")),
        "PatientAge" => Some((0x0010, 0x1010, "AS")),
        "PatientSize" => Some((0x0010, 0x1020, "DS")),
        "PatientWeight" => Some((0x0010, 0x1030, "DS")),
        "EthnicGroup" => Some((0x0010, 0x2160, "SH")),

        // Study Level
        "StudyInstanceUID" => Some((0x0020, 0x000D, "UI")),
        "StudyID" => Some((0x0020, 0x0010, "SH")),
        "AccessionNumber" => Some((0x0008, 0x0050, "SH")),
        "StudyDate" => Some((0x0008, 0x0020, "DA")),
        "StudyTime" => Some((0x0008, 0x0030, "TM")),
        "StudyDescription" => Some((0x0008, 0x1030, "LO")),
        "ReferringPhysicianName" => Some((0x0008, 0x0090, "PN")),
        "InstitutionName" => Some((0x0008, 0x0080, "LO")),
        "ModalitiesInStudy" => Some((0x0008, 0x0061, "CS")),

        // Series Level
        "SeriesInstanceUID" => Some((0x0020, 0x000E, "UI")),
        "SeriesNumber" => Some((0x0020, 0x0011, "IS")),
        "Modality" => Some((0x0008, 0x0060, "CS")),
        "SeriesDescription" => Some((0x0008, 0x103E, "LO")),
        "SeriesDate" => Some((0x0008, 0x0021, "DA")),
        "SeriesTime" => Some((0x0008, 0x0031, "TM")),
        "BodyPartExamined" => Some((0x0018, 0x0015, "CS")),
        "ProtocolName" => Some((0x0018, 0x1030, "LO")),
        "PatientPosition" => Some((0x0018, 0x5100, "CS")),
        "FrameOfReferenceUID" => Some((0x0020, 0x0052, "UI")),
        "Manufacturer" => Some((0x0008, 0x0070, "LO")),
        "ManufacturerModelName" => Some((0x0008, 0x1090, "LO")),

        _ => None,
    }
}

pub fn encode_element(group: u16, elem: u16, vr: &str, mut val: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&group.to_le_bytes());
    out.extend_from_slice(&elem.to_le_bytes());
    out.extend_from_slice(vr.as_bytes());

    // Pad to even length per DICOM standard
    if val.len() % 2 != 0 {
        if vr == "UI" {
            val.push(b'\0');
        } else {
            val.push(b' ');
        }
    }

    match vr {
        "OB" | "OD" | "OF" | "OL" | "OW" | "SQ" | "UC" | "UR" | "UT" | "UN" => {
            out.extend_from_slice(&[0u8, 0u8]); // 2 reserved bytes
            out.extend_from_slice(&(val.len() as u32).to_le_bytes());
        }
        _ => {
            out.extend_from_slice(&(val.len() as u16).to_le_bytes());
        }
    }
    out.extend_from_slice(&val);
    out
}

/// Reconstruct standard DICOM binary (.dcm) bytes from extracted tags, dimensions, and pixel data.
pub fn reconstruct_dicom_bytes(
    patient_tags: &BTreeMap<String, String>,
    study_tags: &BTreeMap<String, String>,
    series_tags: &BTreeMap<String, String>,
    rows: u16,
    cols: u16,
    bits_allocated: u16,
    pixel_data: &[u8],
    instance_id: &str,
) -> Result<Vec<u8>, String> {
    let mut dcm = Vec::with_capacity(128 + 4 + 256 + pixel_data.len());

    // 1. 128-byte preamble + "DICM"
    dcm.extend_from_slice(&[0u8; 128]);
    dcm.extend_from_slice(b"DICM");

    // Determine SOP class and instance UIDs
    let modality = series_tags
        .get("Modality")
        .map(String::as_str)
        .unwrap_or("MR");
    let sop_class_uid = match modality {
        "CT" => "1.2.840.10008.5.1.4.1.1.2",
        _ => "1.2.840.10008.5.1.4.1.1.4", // MR Image Storage default
    };

    let series_uid = series_tags
        .get("SeriesInstanceUID")
        .map(String::as_str)
        .unwrap_or("1.2.826.0.1");

    let sop_instance_uid = format!("{}.{}", series_uid, instance_id.trim_end_matches(".dcm"));

    // 2. Group 0002 elements (File Meta Information)
    let mut g2 = Vec::new();
    g2.extend(encode_element(0x0002, 0x0001, "OB", vec![0x00, 0x01]));
    g2.extend(encode_element(0x0002, 0x0002, "UI", sop_class_uid.as_bytes().to_vec()));
    g2.extend(encode_element(0x0002, 0x0003, "UI", sop_instance_uid.as_bytes().to_vec()));
    g2.extend(encode_element(0x0002, 0x0010, "UI", b"1.2.840.10008.1.2.1".to_vec()));
    g2.extend(encode_element(0x0002, 0x0012, "UI", b"1.2.826.0.1.3680043.9.7134.1".to_vec()));

    let g2_len = g2.len() as u32;
    dcm.extend(encode_element(0x0002, 0x0000, "UL", g2_len.to_le_bytes().to_vec()));
    dcm.extend(g2);

    // 3. Collect all data elements into a sorted map by (group, element)
    let mut elements: BTreeMap<(u16, u16), Vec<u8>> = BTreeMap::new();

    // SOPClassUID and SOPInstanceUID in Group 0008
    elements.insert((0x0008, 0x0016), encode_element(0x0008, 0x0016, "UI", sop_class_uid.as_bytes().to_vec()));
    elements.insert((0x0008, 0x0018), encode_element(0x0008, 0x0018, "UI", sop_instance_uid.as_bytes().to_vec()));

    // Merge patient, study, series tags
    for tags in [patient_tags, study_tags, series_tags] {
        for (k, v) in tags {
            if v.is_empty() {
                continue;
            }
            if let Some((group, elem, vr)) = get_tag_spec(k) {
                elements.insert((group, elem), encode_element(group, elem, vr, v.as_bytes().to_vec()));
            }
        }
    }

    // Group 0028: Image Presentation tags
    elements.insert((0x0028, 0x0002), encode_element(0x0028, 0x0002, "US", 1u16.to_le_bytes().to_vec()));
    elements.insert((0x0028, 0x0004), encode_element(0x0028, 0x0004, "CS", b"MONOCHROME2".to_vec()));
    elements.insert((0x0028, 0x0010), encode_element(0x0028, 0x0010, "US", rows.to_le_bytes().to_vec()));
    elements.insert((0x0028, 0x0011), encode_element(0x0028, 0x0011, "US", cols.to_le_bytes().to_vec()));
    elements.insert((0x0028, 0x0100), encode_element(0x0028, 0x0100, "US", bits_allocated.to_le_bytes().to_vec()));
    elements.insert((0x0028, 0x0101), encode_element(0x0028, 0x0101, "US", bits_allocated.to_le_bytes().to_vec()));
    let high_bit = if bits_allocated > 0 { bits_allocated - 1 } else { 7 };
    elements.insert((0x0028, 0x0102), encode_element(0x0028, 0x0102, "US", high_bit.to_le_bytes().to_vec()));
    elements.insert((0x0028, 0x0103), encode_element(0x0028, 0x0103, "US", 0u16.to_le_bytes().to_vec()));

    // Group 7FE0: Pixel Data
    let pixel_vr = if bits_allocated == 8 { "OB" } else { "OW" };
    elements.insert((0x7FE0, 0x0010), encode_element(0x7FE0, 0x0010, pixel_vr, pixel_data.to_vec()));

    // Write all elements sorted by (group, element)
    for (_tag, elem_bytes) in elements {
        dcm.extend(elem_bytes);
    }

    Ok(dcm)
}

/// Helper to read exactly 2 bytes little-endian as u16.
fn read_u16<R: Read>(reader: &mut R) -> std::io::Result<u16> {
    let mut buf = [0u8; 2];
    reader.read_exact(&mut buf)?;
    Ok(u16::from_le_bytes(buf))
}

/// Helper to read exactly 4 bytes little-endian as u32.
fn read_u32<R: Read>(reader: &mut R) -> std::io::Result<u32> {
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

/// Skip undefined-length sequence or item until delimiter tag is found.
fn skip_undefined_length<R: Read + Seek>(reader: &mut R, delim_elem: u16) -> std::io::Result<()> {
    loop {
        let group = match read_u16(reader) {
            Ok(g) => g,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e),
        };
        let elem = match read_u16(reader) {
            Ok(el) => el,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e),
        };
        let length = read_u32(reader)?;

        if group == 0xFFFE && elem == delim_elem {
            return Ok(());
        }

        if length != 0xFFFF_FFFF {
            reader.seek(SeekFrom::Current(length as i64))?;
        } else if group == 0xFFFE && elem == 0xE000 {
            // Nested undefined length item
            skip_undefined_length(reader, 0xE00D)?;
        }
    }
}

/// Parse DICOM binary header using streaming reader and seeks.
/// Never loads the entire file or pixel data into memory.
pub fn parse_dicom_header(path: &Path) -> Result<DicomMetadata, String> {
    let file = File::open(path).map_err(|e| format!("failed to open {}: {}", path.display(), e))?;
    let mut reader = BufReader::new(file);

    // Read 128 bytes preamble + 4 bytes DICM magic
    let mut preamble = [0u8; 128];
    reader
        .read_exact(&mut preamble)
        .map_err(|e| format!("failed to read DICOM preamble: {}", e))?;

    let mut magic = [0u8; 4];
    reader
        .read_exact(&mut magic)
        .map_err(|e| format!("failed to read DICOM magic: {}", e))?;

    if &magic != b"DICM" {
        return Err("missing DICM magic header".to_string());
    }

    let mut metadata = DicomMetadata::default();
    let mut is_explicit = true; // Group 0x0002 is always Explicit VR Little Endian

    loop {
        let group = match read_u16(&mut reader) {
            Ok(g) => g,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(format!("error reading tag group: {}", e)),
        };

        let elem = match read_u16(&mut reader) {
            Ok(el) => el,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(format!("error reading tag element: {}", e)),
        };

        // All required tags are <= group 0x0020.
        // Stop before PixelData (0x7FE0, 0x0010) or group > 0x0020.
        if group > 0x0020 || (group == 0x7FE0 && elem == 0x0010) {
            break;
        }

        let vl: u32;
        let mut vr_code = [0u8; 2];

        if is_explicit || group == 0x0002 {
            reader
                .read_exact(&mut vr_code)
                .map_err(|e| format!("error reading VR: {}", e))?;

            match &vr_code {
                b"OB" | b"OD" | b"OF" | b"OL" | b"OW" | b"SQ" | b"UC" | b"UR" | b"UT" | b"UN" => {
                    let mut reserved = [0u8; 2];
                    reader
                        .read_exact(&mut reserved)
                        .map_err(|e| format!("error reading reserved bytes: {}", e))?;
                    vl = read_u32(&mut reader)
                        .map_err(|e| format!("error reading 32-bit VL: {}", e))?;
                }
                _ => {
                    vl = read_u16(&mut reader)
                        .map_err(|e| format!("error reading 16-bit VL: {}", e))?
                        as u32;
                }
            }
        } else {
            vl = read_u32(&mut reader).map_err(|e| format!("error reading implicit VL: {}", e))?;
        }

        if vl == 0xFFFF_FFFF {
            // Undefined length sequence
            skip_undefined_length(&mut reader, 0xE0DD)
                .map_err(|e| format!("error skipping undefined sequence: {}", e))?;
            continue;
        }

        // Check if this tag is one of our target metadata tags
        let tag_info = get_tag_info(group, elem);

        // Also check if this is TransferSyntaxUID (0x0002, 0x0010)
        let is_transfer_syntax = group == 0x0002 && elem == 0x0010;

        if tag_info.is_some() || is_transfer_syntax {
            // Read value bytes into memory (limited to vl)
            let mut val_bytes = vec![0u8; vl as usize];
            reader
                .read_exact(&mut val_bytes)
                .map_err(|e| format!("error reading tag value: {}", e))?;

            let val_str = String::from_utf8_lossy(&val_bytes)
                .trim_matches(|c: char| c == ' ' || c == '\0')
                .to_string();

            if is_transfer_syntax {
                // If transfer syntax is Implicit VR Little Endian (1.2.840.10008.1.2)
                if val_str == "1.2.840.10008.1.2" {
                    is_explicit = false;
                }
            }

            if let Some((level, keyword)) = tag_info {
                match level {
                    TagLevel::Patient => {
                        metadata.patient.insert(keyword.to_string(), val_str);
                    }
                    TagLevel::Study => {
                        metadata.study.insert(keyword.to_string(), val_str);
                    }
                    TagLevel::Series => {
                        metadata.series.insert(keyword.to_string(), val_str);
                    }
                }
            }
        } else {
            // Seek past value without reading into memory!
            reader
                .seek(SeekFrom::Current(vl as i64))
                .map_err(|e| format!("error seeking past element (vl={}): {}", vl, e))?;
        }
    }

    Ok(metadata)
}

/// Extract image geometry and pixel data from a DICOM file by streaming up to PixelData (0x7FE0, 0x0010).
pub fn extract_dicom_pixel_data(path: &Path) -> Result<DicomPixelData, String> {
    let file = File::open(path).map_err(|e| format!("failed to open {}: {}", path.display(), e))?;
    let mut reader = BufReader::new(file);

    let mut preamble = [0u8; 128];
    reader
        .read_exact(&mut preamble)
        .map_err(|e| format!("failed to read DICOM preamble: {}", e))?;

    let mut magic = [0u8; 4];
    reader
        .read_exact(&mut magic)
        .map_err(|e| format!("failed to read DICOM magic: {}", e))?;

    if &magic != b"DICM" {
        return Err("missing DICM magic header".to_string());
    }

    let mut is_explicit = true;
    let mut rows: u16 = 0;
    let mut cols: u16 = 0;
    let mut bits_allocated: u16 = 16;
    let mut pixel_data = Vec::new();

    loop {
        let group = match read_u16(&mut reader) {
            Ok(g) => g,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(format!("error reading tag group: {}", e)),
        };

        let elem = match read_u16(&mut reader) {
            Ok(el) => el,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(format!("error reading tag element: {}", e)),
        };

        let vl: u32;
        let mut vr_code = [0u8; 2];

        if is_explicit || group == 0x0002 {
            reader
                .read_exact(&mut vr_code)
                .map_err(|e| format!("error reading VR: {}", e))?;

            match &vr_code {
                b"OB" | b"OD" | b"OF" | b"OL" | b"OW" | b"SQ" | b"UC" | b"UR" | b"UT" | b"UN" => {
                    let mut reserved = [0u8; 2];
                    reader
                        .read_exact(&mut reserved)
                        .map_err(|e| format!("error reading reserved bytes: {}", e))?;
                    vl = read_u32(&mut reader)
                        .map_err(|e| format!("error reading 32-bit VL: {}", e))?;
                }
                _ => {
                    vl = read_u16(&mut reader)
                        .map_err(|e| format!("error reading 16-bit VL: {}", e))?
                        as u32;
                }
            }
        } else {
            vl = read_u32(&mut reader).map_err(|e| format!("error reading implicit VL: {}", e))?;
        }

        if vl == 0xFFFF_FFFF {
            skip_undefined_length(&mut reader, 0xE0DD)
                .map_err(|e| format!("error skipping undefined sequence: {}", e))?;
            continue;
        }

        if group == 0x0002 && elem == 0x0010 {
            let mut val_bytes = vec![0u8; vl as usize];
            reader.read_exact(&mut val_bytes).map_err(|e| e.to_string())?;
            let val_str = String::from_utf8_lossy(&val_bytes)
                .trim_matches(|c: char| c == ' ' || c == '\0')
                .to_string();
            if val_str == "1.2.840.10008.1.2" {
                is_explicit = false;
            }
        } else if group == 0x0028 && elem == 0x0010 {
            if vl >= 2 {
                rows = read_u16(&mut reader).map_err(|e| e.to_string())?;
                if vl > 2 {
                    reader
                        .seek(SeekFrom::Current((vl - 2) as i64))
                        .map_err(|e| e.to_string())?;
                }
            } else {
                reader
                    .seek(SeekFrom::Current(vl as i64))
                    .map_err(|e| e.to_string())?;
            }
        } else if group == 0x0028 && elem == 0x0011 {
            if vl >= 2 {
                cols = read_u16(&mut reader).map_err(|e| e.to_string())?;
                if vl > 2 {
                    reader
                        .seek(SeekFrom::Current((vl - 2) as i64))
                        .map_err(|e| e.to_string())?;
                }
            } else {
                reader
                    .seek(SeekFrom::Current(vl as i64))
                    .map_err(|e| e.to_string())?;
            }
        } else if group == 0x0028 && elem == 0x0100 {
            if vl >= 2 {
                bits_allocated = read_u16(&mut reader).map_err(|e| e.to_string())?;
                if vl > 2 {
                    reader
                        .seek(SeekFrom::Current((vl - 2) as i64))
                        .map_err(|e| e.to_string())?;
                }
            } else {
                reader
                    .seek(SeekFrom::Current(vl as i64))
                    .map_err(|e| e.to_string())?;
            }
        } else if group == 0x7FE0 && elem == 0x0010 {
            pixel_data = vec![0u8; vl as usize];
            reader
                .read_exact(&mut pixel_data)
                .map_err(|e| format!("error reading pixel data: {}", e))?;
            break;
        } else {
            reader
                .seek(SeekFrom::Current(vl as i64))
                .map_err(|e| e.to_string())?;
        }
    }

    Ok(DicomPixelData {
        rows,
        cols,
        bits_allocated,
        data: pixel_data,
    })
}

/// Save or cumulatively merge tags into `<draft_dir>/<id>.json`.
/// Creates `draft_dir` if it does not exist.
pub fn save_or_merge_draft(
    draft_dir: &Path,
    id: &str,
    new_tags: &BTreeMap<String, String>,
) -> Result<PathBuf, String> {
    fs::create_dir_all(draft_dir)
        .map_err(|e| format!("failed to create draft dir {}: {}", draft_dir.display(), e))?;

    let file_path = draft_dir.join(format!("{}.json", id));

    let mut map: serde_json::Map<String, serde_json::Value> = if file_path.exists() {
        let content = fs::read_to_string(&file_path)
            .map_err(|e| format!("failed to read existing draft {}: {}", file_path.display(), e))?;
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        serde_json::Map::new()
    };

    // Merge tags
    for (k, v) in new_tags {
        if v.is_empty() {
            continue;
        }

        if k == "ModalitiesInStudy" {
            if let Some(existing_val) = map.get(k).and_then(|v| v.as_str()) {
                let mut modalities: Vec<&str> = existing_val.split('\\').map(str::trim).collect();
                for mod_part in v.split('\\').map(str::trim) {
                    if !mod_part.is_empty() && !modalities.contains(&mod_part) {
                        modalities.push(mod_part);
                    }
                }
                map.insert(
                    k.clone(),
                    serde_json::Value::String(modalities.join("\\")),
                );
            } else {
                map.insert(k.clone(), serde_json::Value::String(v.clone()));
            }
        } else {
            // If existing doesn't exist or is empty string, update
            let should_update = match map.get(k) {
                None => true,
                Some(existing_val) => {
                    existing_val.as_str().map(|s| s.is_empty()).unwrap_or(true)
                }
            };
            if should_update {
                map.insert(k.clone(), serde_json::Value::String(v.clone()));
            }
        }
    }

    let json_str = serde_json::to_string_pretty(&map)
        .map_err(|e| format!("failed to serialize draft json: {}", e))?;

    fs::write(&file_path, json_str)
        .map_err(|e| format!("failed to write draft {}: {}", file_path.display(), e))?;

    Ok(file_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sample_dicom_header() {
        let sample_path = Path::new("src/tester/generate/data/1.3.12.2.1107.5.2.19.45594.2018071915225876779003334.dcm");
        let meta = parse_dicom_header(sample_path).expect("failed to parse sample DICOM");

        // Verify Patient tags
        assert_eq!(meta.patient.get("PatientID").map(String::as_str), Some("J3 DE P5 C5"));
        assert_eq!(meta.patient.get("PatientSex").map(String::as_str), Some("O"));
        assert_eq!(meta.patient.get("PatientAge").map(String::as_str), Some("018Y"));

        // Verify Study tags
        assert_eq!(meta.study.get("StudyInstanceUID").map(String::as_str), Some("1.2.826.0.1.4978912.2142.20180719150610"));
        assert_eq!(meta.study.get("StudyDate").map(String::as_str), Some("20180719"));

        // Verify Series tags
        assert_eq!(meta.series.get("SeriesInstanceUID").map(String::as_str), Some("1.3.12.2.1107.5.2.19.45594.2018071915225174195702396.0.0.0"));
        assert_eq!(meta.series.get("Modality").map(String::as_str), Some("MR"));
        assert_eq!(meta.series.get("Manufacturer").map(String::as_str), Some("SIEMENS"));
    }

    #[test]
    fn test_extract_dicom_pixel_data() {
        let sample_path = Path::new("src/tester/generate/data/1.3.12.2.1107.5.2.19.45594.2018071915225876779003334.dcm");
        let px = extract_dicom_pixel_data(sample_path).expect("failed to extract pixel data");
        assert_eq!(px.rows, 256);
        assert_eq!(px.cols, 256);
        assert_eq!(px.bits_allocated, 16);
        assert_eq!(px.data.len(), 256 * 256 * 2);
    }

    #[test]
    fn test_reconstruct_dicom_bytes() {
        let mut patient_tags = BTreeMap::new();
        patient_tags.insert("PatientID".to_string(), "PAT123".to_string());
        patient_tags.insert("PatientName".to_string(), "Smith^Jane".to_string());

        let mut study_tags = BTreeMap::new();
        study_tags.insert("StudyInstanceUID".to_string(), "1.2.3.4.5".to_string());
        study_tags.insert("StudyDate".to_string(), "20240101".to_string());

        let mut series_tags = BTreeMap::new();
        series_tags.insert("SeriesInstanceUID".to_string(), "1.2.3.4.5.6".to_string());
        series_tags.insert("Modality".to_string(), "MR".to_string());

        let dummy_pixels = vec![42u8; 64 * 64 * 2];
        let bytes = reconstruct_dicom_bytes(
            &patient_tags,
            &study_tags,
            &series_tags,
            64,
            64,
            16,
            &dummy_pixels,
            "instance_001.dcm",
        )
        .expect("reconstruct dicom bytes");

        let temp_file = std::env::temp_dir().join(format!("test_reconstruct_{}.dcm", std::process::id()));
        fs::write(&temp_file, bytes).unwrap();

        let parsed = parse_dicom_header(&temp_file).expect("parse reconstructed dicom");
        assert_eq!(parsed.patient.get("PatientID").map(String::as_str), Some("PAT123"));
        assert_eq!(parsed.patient.get("PatientName").map(String::as_str), Some("Smith^Jane"));
        assert_eq!(parsed.study.get("StudyInstanceUID").map(String::as_str), Some("1.2.3.4.5"));
        assert_eq!(parsed.series.get("Modality").map(String::as_str), Some("MR"));

        let px = extract_dicom_pixel_data(&temp_file).expect("extract pixel data from reconstructed");
        assert_eq!(px.rows, 64);
        assert_eq!(px.cols, 64);
        assert_eq!(px.bits_allocated, 16);
        assert_eq!(px.data, dummy_pixels);

        let _ = fs::remove_file(temp_file);
    }

    #[test]
    fn test_save_or_merge_draft() {
        let temp_dir = std::env::temp_dir().join(format!("test_draft_merge_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);

        let mut initial_tags = BTreeMap::new();
        initial_tags.insert("PatientID".to_string(), "PAT_001".to_string());
        initial_tags.insert("PatientName".to_string(), "John Doe".to_string());

        let path = save_or_merge_draft(&temp_dir, "patient_001", &initial_tags).unwrap();
        assert!(path.exists());

        // Read and check
        let content: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(content["PatientID"], "PAT_001");
        assert_eq!(content["PatientName"], "John Doe");

        // Merge with additional tag
        let mut second_tags = BTreeMap::new();
        second_tags.insert("PatientBirthDate".to_string(), "19800101".to_string());
        save_or_merge_draft(&temp_dir, "patient_001", &second_tags).unwrap();

        let updated: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(updated["PatientID"], "PAT_001");
        assert_eq!(updated["PatientName"], "John Doe");
        assert_eq!(updated["PatientBirthDate"], "19800101");

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
