### **1\. Patient Level**

Identifies the individual patient. These tags should remain consistent across all studies for the same person.

| Tag | Keyword | VR | Description |
| :---- | :---- | :---- | :---- |
| (0010,0020) | PatientID | LO | Primary identifier for the patient (Primary Key at this level) |
| (0010,0010) | PatientName | PN | Full name of the patient |
| (0010,0030) | PatientBirthDate | DA | Date of birth (YYYYMMDD) |
| (0010,0040) | PatientSex | CS | Biological sex (M, F, O) |
| (0010,1010) | PatientAge | AS | Age at the time of the study (e.g., 045Y) |
| (0010,1020) | PatientSize | DS | Height/length in meters |
| (0010,1030) | PatientWeight | DS | Weight in kilograms |
| (0010,2160) | EthnicGroup | SH | Ethnic group of the patient |

### **2\. Study Level**

Represents a single imaging exam or visit for a patient. One patient can have multiple studies.

| Tag | Keyword | VR | Description |
| :---- | :---- | :---- | :---- |
| (0020,000D) | StudyInstanceUID | UI | Globally unique identifier for the study (Primary Key) |
| (0020,0010) | StudyID | SH | User- or equipment-generated study identifier |
| (0008,0050) | AccessionNumber | SH | RIS-generated order/accession number |
| (0008,0020) | StudyDate | DA | Date the study started (YYYYMMDD) |
| (0008,0030) | StudyTime | TM | Time the study started (HHMMSS.FFFFFF) |
| (0008,1030) | StudyDescription | LO | Institution-generated description of the exam |
| (0008,0090) | ReferringPhysicianName | PN | Name of the referring physician |
| (0008,0080) | InstitutionName | LO | Name of the hospital or imaging center |
| (0008,0061) | ModalitiesInStudy | CS | All modalities present in the study (if populated) |

### **3\. Series Level**

Represents a distinct set of images or data acquired during a study (e.g., an axial T1 sequence or a localizer). One study contains multiple series.

| Tag | Keyword | VR | Description |
| :---- | :---- | :---- | :---- |
| (0020,000E) | SeriesInstanceUID | UI | Globally unique identifier for the series (Primary Key) |
| (0020,0011) | SeriesNumber | IS | Number identifying this series within the study |
| (0008,0060) | Modality | CS | Type of equipment/acquisition (CT, MR, US, CR, DX, etc.) |
| (0008,103E) | SeriesDescription | LO | Description of the series/protocol |
| (0008,0021) | SeriesDate | DA | Date the series started (YYYYMMDD) |
| (0008,0031) | SeriesTime | TM | Time the series started (HHMMSS.FFFFFF) |
| (0018,0015) | BodyPartExamined | CS | Anatomical region examined (CHEST, HEAD, ABDOMEN, etc.) |
| (0018,1030) | ProtocolName | LO | Name of the acquisition protocol used |
| (0018,5100) | PatientPosition | CS | Patient position relative to the scanner (HFS, FFS, HFP, etc.) |
| (0020,0052) | FrameOfReferenceUID | UI | Identifies the spatial coordinate system for the series |
| (0008,0070) | Manufacturer | LO | Scanner manufacturer (e.g., SIEMENS, GE, PHILIPS) |
| (0008,1090) | ManufacturerModelName | LO | Scanner model name |

