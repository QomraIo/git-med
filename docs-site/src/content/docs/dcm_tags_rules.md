---
title: DICOM Tag Rules
description: Information Entity (IE) hierarchy and tag categorization rules for DICOM files
---

# DCM Tags Rules (Draft)

This is a draft for DCM tags grouping and classification.
We classify tags based on:
1. IE (Information Entity) levels:
   - Patient
   - Study
   - Series
   - Image
2. The reason for their inclusion in this group:
   - Identifiers: Tags that uniquely identify the IE or are commonly used for identification.
   - Descriptive: Tags that describe the IE or provide additional context.

---

## Patient Level

### Identifiers
- `(0010,0010)` PatientName
- `(0010,0020)` PatientID
- `(0010,0021)` IssuerOfPatientID

### Descriptive
- `(0010,0030)` PatientBirthDate
- `(0010,0040)` PatientSex
- `(0010,1010)` PatientAge
- `(0010,1020)` PatientSize
- `(0010,1030)` PatientWeight

---

## Study Level

### Identifiers
- `(0020,000D)` StudyInstanceUID
- `(0020,0010)` StudyID
- `(0008,0050)` AccessionNumber

### Descriptive
- `(0008,0020)` StudyDate
- `(0008,0030)` StudyTime
- `(0008,1030)` StudyDescription

---

## Series Level

### Identifiers
- `(0020,000E)` SeriesInstanceUID
- `(0020,0011)` SeriesNumber

### Descriptive
- `(0008,0060)` Modality
- `(0008,103E)` SeriesDescription
- `(0018,0015)` BodyPartExamined
- `(0018,1030)` ProtocolName
- `(0020,0032)` ImagePositionPatient
- `(0020,0037)` ImageOrientationPatient
- `(0028,0030)` PixelSpacing
- `(0018,0050)` SliceThickness
- `(0018,0088)` SpacingBetweenSlices

---

## Image (Instance) Level

### Identifiers
- `(0008,0018)` SOPInstanceUID
- `(0020,0013)` InstanceNumber

### Descriptive
- `(0008,0016)` SOPClassUID
- `(0008,0023)` ContentDate
- `(0008,0033)` ContentTime
- `(0028,0010)` Rows
- `(0028,0011)` Columns
- `(0028,0100)` BitsAllocated
- `(0028,0101)` BitsStored
- `(0028,0102)` HighBit
- `(0028,0103)` PixelRepresentation
- `(0028,1050)` WindowCenter
- `(0028,1051)` WindowWidth
- `(0028,1052)` RescaleIntercept
- `(0028,1053)` RescaleSlope
