use std::path::Path;

use crate::files::dicom::extract_dicom_pixel_data;

pub const NIFTI2_HEADER_SIZE: usize = 540;
pub const NIFTI2_VOX_OFFSET: i64 = 544; // 540 header + 4 extension bytes

/// Construct a standard compliant NIfTI-2 binary image from 2D pixel data.
pub fn create_nifti2_from_pixel_data(
    rows: u16,
    cols: u16,
    bits_allocated: u16,
    pixel_data: &[u8],
) -> Vec<u8> {
    let mut header = vec![0u8; NIFTI2_HEADER_SIZE];

    // 1. sizeof_hdr (int32 = 540)
    header[0..4].copy_from_slice(&(540i32).to_le_bytes());

    // 2. magic (char[8] = "n+2\0\r\n\x1a\n")
    header[4..12].copy_from_slice(b"n+2\0\r\n\x1a\n");

    // 3. datatype (int16: 4 = DT_SIGNED_SHORT / int16, 2 = DT_UNSIGNED_CHAR / uint8)
    let datatype: i16 = if bits_allocated == 8 { 2 } else { 4 };
    header[12..14].copy_from_slice(&datatype.to_le_bytes());

    // 4. bitpix (int16)
    header[14..16].copy_from_slice(&(bits_allocated as i16).to_le_bytes());

    // 5. dim[8] (int64_t * 8)
    // dim[0] = 3 (3D image), dim[1] = cols, dim[2] = rows, dim[3] = 1, rest 1
    let dims: [i64; 8] = [3, cols as i64, rows as i64, 1, 1, 1, 1, 1];
    for (i, &d) in dims.iter().enumerate() {
        let offset = 16 + i * 8;
        header[offset..offset + 8].copy_from_slice(&d.to_le_bytes());
    }

    // 6. intent_p1..p3 (3 * double = 24 bytes, default 0.0)
    // Already zeroed

    // 7. pixdim[8] (8 * double = 64 bytes)
    let pixdims: [f64; 8] = [0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
    for (i, &pd) in pixdims.iter().enumerate() {
        let offset = 104 + i * 8;
        header[offset..offset + 8].copy_from_slice(&pd.to_le_bytes());
    }

    // 8. vox_offset (int64_t = 544)
    header[168..176].copy_from_slice(&NIFTI2_VOX_OFFSET.to_le_bytes());

    // 9. scl_slope (double = 1.0)
    header[176..184].copy_from_slice(&(1.0f64).to_le_bytes());

    // 10. scl_inter (double = 0.0) -> already zeroed
    // 11. cal_max, cal_min (double) -> already zeroed
    // 12. slice_duration, toffset, slice_start, slice_end -> already zeroed

    // 13. descrip (80 bytes)
    let descrip = b"git-med nifti-2";
    header[240..240 + descrip.len()].copy_from_slice(descrip);

    // 14. aux_file (24 bytes) -> already zeroed

    // 15. qform_code and sform_code (int32 = 1, NIFTI_XFORM_SCANNER_ANAT)
    header[344..348].copy_from_slice(&(1i32).to_le_bytes());
    header[348..352].copy_from_slice(&(1i32).to_le_bytes());

    // 16. quatern_b, c, d, qoffset_x, y, z -> already zeroed

    // 17. srow_x, srow_y, srow_z (identity affine transform)
    let srow_x: [f64; 4] = [1.0, 0.0, 0.0, 0.0];
    let srow_y: [f64; 4] = [0.0, 1.0, 0.0, 0.0];
    let srow_z: [f64; 4] = [0.0, 0.0, 1.0, 0.0];
    for (i, &v) in srow_x.iter().enumerate() {
        let offset = 400 + i * 8;
        header[offset..offset + 8].copy_from_slice(&v.to_le_bytes());
    }
    for (i, &v) in srow_y.iter().enumerate() {
        let offset = 432 + i * 8;
        header[offset..offset + 8].copy_from_slice(&v.to_le_bytes());
    }
    for (i, &v) in srow_z.iter().enumerate() {
        let offset = 464 + i * 8;
        header[offset..offset + 8].copy_from_slice(&v.to_le_bytes());
    }

    // 18. xyzt_units (int32 = 2, NIFTI_UNITS_MM)
    header[500..504].copy_from_slice(&(2i32).to_le_bytes());

    // 19. 4 extension bytes
    let extension = [0u8; 4];

    // Combine header (540B) + extension (4B) + pixel data
    let mut out = Vec::with_capacity(NIFTI2_HEADER_SIZE + extension.len() + pixel_data.len());
    out.extend_from_slice(&header);
    out.extend_from_slice(&extension);
    out.extend_from_slice(pixel_data);

    out
}

/// Transform a `.dcm` file into an uncompressed NIfTI-2 binary image.
pub fn dicom_to_nifti2(dcm_path: &Path) -> Result<Vec<u8>, String> {
    let px = extract_dicom_pixel_data(dcm_path)?;
    Ok(create_nifti2_from_pixel_data(
        px.rows,
        px.cols,
        px.bits_allocated,
        &px.data,
    ))
}

#[derive(Debug, Clone)]
pub struct Nifti2PixelData<'a> {
    pub rows: u16,
    pub cols: u16,
    pub bits_allocated: u16,
    pub pixel_data: &'a [u8],
}

/// Extract image dimensions and pixel data slice from a NIfTI-2 binary payload.
pub fn extract_nifti2_pixel_data(bytes: &[u8]) -> Result<Nifti2PixelData<'_>, String> {
    if bytes.len() < NIFTI2_VOX_OFFSET as usize {
        return Err(format!(
            "NIfTI-2 payload too short ({} bytes, expected at least {})",
            bytes.len(),
            NIFTI2_VOX_OFFSET
        ));
    }

    let sizeof_hdr = i32::from_le_bytes(
        bytes[0..4]
            .try_into()
            .map_err(|e| format!("failed to read sizeof_hdr: {}", e))?,
    );
    if sizeof_hdr != 540 {
        return Err(format!(
            "invalid NIfTI-2 sizeof_hdr {} (expected 540)",
            sizeof_hdr
        ));
    }

    if &bytes[4..12] != b"n+2\0\r\n\x1a\n" {
        return Err("invalid NIfTI-2 magic signature".to_string());
    }

    let bitpix = i16::from_le_bytes(
        bytes[14..16]
            .try_into()
            .map_err(|e| format!("failed to read bitpix: {}", e))?,
    ) as u16;

    let cols = i64::from_le_bytes(
        bytes[24..32]
            .try_into()
            .map_err(|e| format!("failed to read dim1 (cols): {}", e))?,
    ) as u16;

    let rows = i64::from_le_bytes(
        bytes[32..40]
            .try_into()
            .map_err(|e| format!("failed to read dim2 (rows): {}", e))?,
    ) as u16;

    let vox_offset = i64::from_le_bytes(
        bytes[168..176]
            .try_into()
            .map_err(|e| format!("failed to read vox_offset: {}", e))?,
    ) as usize;

    if bytes.len() < vox_offset {
        return Err(format!(
            "NIfTI-2 byte length {} is smaller than vox_offset {}",
            bytes.len(),
            vox_offset
        ));
    }

    let pixel_data = &bytes[vox_offset..];

    Ok(Nifti2PixelData {
        rows,
        cols,
        bits_allocated: bitpix,
        pixel_data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_nifti2_header() {
        let dummy_pixels = vec![0u8; 128 * 128 * 2];
        let nifti_bytes = create_nifti2_from_pixel_data(128, 128, 16, &dummy_pixels);

        assert_eq!(nifti_bytes.len(), 544 + dummy_pixels.len());

        // Verify sizeof_hdr = 540
        let sizeof_hdr = i32::from_le_bytes(nifti_bytes[0..4].try_into().unwrap());
        assert_eq!(sizeof_hdr, 540);

        // Verify magic = b"n+2\0\r\n\x1a\n"
        assert_eq!(&nifti_bytes[4..12], b"n+2\0\r\n\x1a\n");

        // Verify vox_offset = 544
        let vox_offset = i64::from_le_bytes(nifti_bytes[168..176].try_into().unwrap());
        assert_eq!(vox_offset, 544);

        // Verify dimensions
        let dim0 = i64::from_le_bytes(nifti_bytes[16..24].try_into().unwrap());
        let dim1 = i64::from_le_bytes(nifti_bytes[24..32].try_into().unwrap());
        let dim2 = i64::from_le_bytes(nifti_bytes[32..40].try_into().unwrap());
        assert_eq!(dim0, 3);
        assert_eq!(dim1, 128);
        assert_eq!(dim2, 128);
    }

    #[test]
    fn test_dicom_to_nifti2() {
        let sample_path = Path::new("src/tester/generate/data/1.3.12.2.1107.5.2.19.45594.2018071915225876779003334.dcm");
        let nifti_bytes = dicom_to_nifti2(sample_path).expect("failed to convert dcm to nifti2");

        assert_eq!(nifti_bytes.len(), 544 + 256 * 256 * 2);

        let sizeof_hdr = i32::from_le_bytes(nifti_bytes[0..4].try_into().unwrap());
        assert_eq!(sizeof_hdr, 540);

        let dim1 = i64::from_le_bytes(nifti_bytes[24..32].try_into().unwrap());
        let dim2 = i64::from_le_bytes(nifti_bytes[32..40].try_into().unwrap());
        assert_eq!(dim1, 256);
        assert_eq!(dim2, 256);

        // Test roundtrip extraction
        let extracted = extract_nifti2_pixel_data(&nifti_bytes).expect("extract pixel data");
        assert_eq!(extracted.rows, 256);
        assert_eq!(extracted.cols, 256);
        assert_eq!(extracted.bits_allocated, 16);
        assert_eq!(extracted.pixel_data.len(), 256 * 256 * 2);
    }
}

