---
title: Overview & Quickstart
description: Getting started with git-med installation, build, and basic usage
---

`git-med` is an experimental distributed version control system (DVCS) designed to power biomedical research workflows and modern PACS/VNA (Vendor Neutral Archive) archives.

---

## Prerequisites

- **Rust & Cargo**: Rust 1.85+ with 2024 edition support ([Install Rust](https://www.rust-lang.org/tools/install))

---

## Build Commands

Compile `git-med` from source:

```bash
# Debug build (binary output: ./target/debug/git-med)
cargo build

# Optimized release build (binary output: ./target/release/git-med)
cargo build --release

# Install binary to ~/.cargo/bin (enables `git-med` globally on PATH)
cargo install --path .
```

---

## Basic Workflow & Run Commands

### 1. Initialize a Medical Repository
```bash
git-med init my_dataset
cd my_dataset
```

### 2. Generate or Add Medical Imaging Data
Generate synthetic DICOM, BraTS, or QOMRA datasets for testing:
```bash
git-med generate dicom ./data 2 1 3
```

### 3. Stage and Commit
`git-med` parses medical tags and extracts metadata into the staging index while storing raw binary images into native LFS:
```bash
git-med add
git-med commit -m "Initial medical dataset import"
```

### 4. Switch Between Imaging Views
Reorganize the physical layout on disk to match different clinical and machine learning standards:
```bash
# Query active view (defaults to qomra)
git-med view

# Transform workspace layout into DICOM SOP hierarchy
git-med view dicom

# Transform workspace layout into BraTS NIfTI structure
git-med view brats
```

### 5. Branching and Remote Synchronization
```bash
# Create and check out a feature branch
git-med branch feature-cohort

# Configure remote forge server
git-med remote add origin http://localhost:3000/alice/my_dataset.git

# Authenticate with git-med-forge
git-med login --server http://localhost:3000 --username alice --password secret

# Push dataset metadata and binary objects to server
git-med push origin main
```

---

## Running Unit and Integration Tests

```bash
cargo test
```
