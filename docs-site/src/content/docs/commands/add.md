---
title: git-med add
description: Scan workspace, extract medical metadata, and stage large binaries into LFS
---

## How to use
`git-med add`

## What
Scans the repository according to the active medical imaging view (e.g., QOMRA, DICOM, BraTS), extracts domain metadata into the staging tree, and routes large binary payloads into native LFS storage.

## Git similarity
| Aspect | git-med | Git | Key Differences |
| --- | --- | --- | --- |
| Invocation Scope | `git-med add` scans the entire repository | `git add <path>` requires specific file/directory paths or `.` | `git-med add` operates globally on the workspace guided by the active medical view parser. |
| Parsing Behavior | Domain-aware metadata parsing (extracts DICOM tags, NIfTI volumes, QOMRA attributes) | Byte-level literal staging | `git-med` interprets medical file contents and builds a semantic attribute hierarchy rather than treating files as opaque blobs. |
| Large File Handling | Built-in native LFS: raw imaging payloads are hashed with BLAKE3 and staged into `.git-med/lfs/objects/` | Stores files directly in Git object database unless third-party Git LFS extension is configured | LFS handling is a native, automatic core feature of `git-med`. |
| Concurrency Control | Acquires `.git-med/index.lock` during indexing | Acquires `.git/index.lock` | Both protect the index from concurrent modification during staging. |
| Ignore Rules | Respects `.gitignore` rules | Respects `.gitignore` rules | Both filter ignored patterns before staging. |
