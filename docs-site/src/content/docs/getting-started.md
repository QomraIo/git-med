---
title: Overview & Quickstart
description: Getting started with git-med installation, build, and basic usage
---

`git-med` is an experimental distributed version control system (DVCS) designed to power biomedical research workflows and modern PACS/VNA (Vendor Neutral Archive) archives.

---

## Installation

### Option 1: Install Prebuilt Binaries (Recommended)

Prebuilt binaries are published with each release on the [GitHub Releases page](https://github.com/QomraIo/git-med/releases).

#### Ubuntu / Linux (x86_64)
```bash
# Download latest Linux release archive
curl -LO https://github.com/QomraIo/git-med/releases/latest/download/git-med-linux-x86_64.tar.gz

# Extract archive
tar -xzf git-med-linux-x86_64.tar.gz

# Install to /usr/local/bin
sudo install -m 755 git-med /usr/local/bin/
```

#### macOS (Apple Silicon ARM64)
```bash
# Download latest macOS Apple Silicon archive
curl -LO https://github.com/QomraIo/git-med/releases/latest/download/git-med-macos-arm64.tar.gz

# Extract archive
tar -xzf git-med-macos-arm64.tar.gz

# Install to /usr/local/bin
sudo install -m 755 git-med /usr/local/bin/

# Clear macOS quarantine flag if prompted
xattr -d com.apple.quarantine /usr/local/bin/git-med
```

---

### Option 2: Build from Source

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

## Setting Up & Running the Server (`git-med-forge`)

Remote commands (`login`, `remote create`, `push`, `pull`, `clone`) talk to a running [`git-med-forge`](https://github.com/QomraIo/git-med-forge) server. Run it on port `8080` (so Next.js/Fabric can use `3000` alongside it).

### Option A: Docker (Fastest for Local Testing)
```bash
docker pull ghcr.io/qomraio/git-med-forge:latest

# In-memory database + standalone auth fallback on port 8080
docker run -d -p 8080:8080 \
  -e GIT_MED_FORGE_PORT=8080 \
  -e GIT_MED_FORGE_MEMORY_DB=true \
  -e GIT_MED_FORGE_AUTH0_ENABLED=false \
  -v $(pwd)/forge-repos:/data/repositories \
  -e GIT_MED_FORGE_DATA_DIR=/data/repositories \
  --name git-med-forge \
  ghcr.io/qomraio/git-med-forge:latest
```

### Option B: Build and Run from Source
Prerequisites: Rust `1.85+`, `pkg-config`, and `libssl-dev` (on Linux).

```bash
git clone https://github.com/QomraIo/git-med-forge.git
cd git-med-forge
cargo build --release

# Local development (in-memory DB, standalone auth, local disk repos on port 8080)
./target/release/git-med-forge \
  --port 8080 \
  --use-memory-db true \
  --auth0-enabled false \
  --data-dir ./data/repositories
```

For production/persistent storage, configure `.env` with `GIT_MED_FORGE_PORT=8080`, `DSQL_CLUSTER_ENDPOINT`, `S3_BUCKET_NAME`, `AWS_REGION`, `AUTH0_DOMAIN`, and `AUTH0_CLIENT_ID`, then start `./target/release/git-med-forge`.

### Verify Server Health
```bash
curl http://localhost:8080/health
# git-med-forge ok
```

---

## Basic Workflow & Run Commands

### 1. Initialize a Medical Repository
```bash
git-med init my_dataset
cd my_dataset
```

### 2. Generate or Add Medical Imaging Data
Generate synthetic DICOM, BraTS, or SMART datasets directly into the current repository directory for testing:
```bash
git-med generate dicom . 2 1 3
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
# Query active view (defaults to smart)
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

# Authenticate with git-med-forge
git-med login --server http://localhost:8080 --username alice --password secret

# Initialize the repository on the remote server
git-med remote create http://localhost:8080/alice/my_dataset.git

# Configure remote forge server
git-med remote add origin http://localhost:8080/alice/my_dataset.git

# Push dataset metadata and binary objects to server
git-med push origin main
```

---

## Running Unit and Integration Tests

```bash
cargo test
```
