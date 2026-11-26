# git-med generate

## How to use
`git-med generate <brats|dicom|qomra|all> <output_dir> [n_subjects] [study_per_subjects] [file_per_study]`

## What
Generates synthetic biomedical imaging datasets (BraTS NIfTI files, DICOM series with standard metadata tags, or QOMRA JSON hierarchies) for testing, development, and validation.

## Git similarity
| Aspect | git-med | Git | Key Differences |
| --- | --- | --- | --- |
| Core Concept | Built-in medical dataset generator | N/A | Git has no built-in data generator; `git-med` provides native synthetic medical data creation for development and benchmarks. |
| Supported Modalities | BraTS (NIfTI), DICOM (SOP series), QOMRA (JSON metadata hierarchies) | N/A | Tailored specifically to biomedical research data structures. |
| Parameter Customization | Configures subject counts, studies per subject, and files per series | N/A | Enables deterministic scalability testing for large datasets. |
