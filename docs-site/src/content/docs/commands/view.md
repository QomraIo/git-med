---
title: git-med view
description: Display or transform dataset layout between medical imaging formats
---

## How to use
`git-med view <qomra|dicom|brats>`

## What
Displays the active dataset projection view or transforms the working directory file layout between medical imaging representations (such as QOMRA JSON, DICOM series, or BraTS NIfTI) without modifying underlying raw data.

## Git similarity
| Aspect | git-med | Git | Key Differences |
| --- | --- | --- | --- |
| Core Concept | Dynamic multi-format projection of medical imaging data | Literal, static filesystem snapshot tracking | Git has no equivalent. Git tracks static file paths, whereas `git-med` can reorganize files on disk according to domain schemas. |
| Working Tree Impact | Rewrites and re-materializes physical directory structures to conform to the requested medical standard | Working files remain strictly in whatever folder hierarchy was committed | Allows biomedical researchers to switch between tool-specific formats (e.g. DICOM for PACS viewers, BraTS for deep learning pipelines) seamlessly. |
| State Tracking | Active view saved in `.git-med/VIEW` | N/A | Persistent configuration file tracks currently active representation. |
