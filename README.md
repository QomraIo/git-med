# git-med - pre-alpha 
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-2024%20edition-orange.svg)](https://www.rust-lang.org)

`git-med` is an experimental/evolving distributed version control system (DVCS). We use it to power Qomra Datasets. We are using this project to experiment with ideas. 

## Ideas we are exploring:
- Import data from any format and interface it in any format
- Versioning for AI and human workflows 
- Switch between diffrent standerds like DICOM, BIDS ...etc.
- Lazy download (VFS, FUSE, NFS, ...)
- Compression (xet, atlas based compression,...)
- File format converter with zero copy
- Fast, secure, scalable and open. 

In current version we opted for building everything thing from scratch, make the architecture modular, write parts we need to understand deeply by hand and prototype the rest with LLMs. The goal is to evolve this system to power future PACS/VNA (Vendor Neutral Archive) systems and AI research tools.

We are considering transforming this to be a git extension/plugin or at least compatible with git.

Mainly, Antigravity with Gemini 3.8 High was used. Claude Opus 4.6 was occasionally for review.

---

## Installation

### Option 1: Install Prebuilt Binaries (Recommended)

Prebuilt standalone binaries are available on the [Releases page](https://github.com/QomraIo/git-med/releases).

#### Ubuntu / Linux (x86_64)
```bash
# 1. Download the latest Linux archive
curl -LO https://github.com/QomraIo/git-med/releases/latest/download/git-med-linux-x86_64.tar.gz

# 2. Extract the archive
tar -xzf git-med-linux-x86_64.tar.gz

# 3. Install to /usr/local/bin (system-wide)
sudo install -m 755 git-med /usr/local/bin/

# Or install for the current user without sudo:
mkdir -p ~/.local/bin && cp git-med ~/.local/bin/
```

#### macOS (Apple Silicon ARM64)
```bash
# 1. Download the latest macOS Apple Silicon archive
curl -LO https://github.com/QomraIo/git-med/releases/latest/download/git-med-macos-arm64.tar.gz

# 2. Extract the archive
tar -xzf git-med-macos-arm64.tar.gz

# 3. Install to /usr/local/bin (system-wide)
sudo install -m 755 git-med /usr/local/bin/

# 4. If macOS Gatekeeper displays an untrusted developer warning:
xattr -d com.apple.quarantine /usr/local/bin/git-med
```

#### Verify Installation
```bash
git-med whoami
```

---

### Option 2: Build from Source

#### Prerequisites
- [Rust & Cargo](https://www.rust-lang.org/tools/install) (Rust 1.85+ with 2024 edition support)

#### Build Commands
```bash
# Build debug binary (located in ./target/debug/git-med)
cargo build

# Build optimized release binary (located in ./target/release/git-med)
cargo build --release

# Install binary directly to ~/.cargo/bin (enables `git-med` on PATH)
cargo install --path .
```

---

## Server Setup (`git-med-forge`)

Before pushing or cloning, start the companion [`git-med-forge`](https://github.com/QomraIo/git-med-forge) server on port `8080`:

```bash
# Option A: Docker (in-memory DB + standalone auth for local testing)
docker run -d -p 8080:8080 \
  -e GIT_MED_FORGE_PORT=8080 \
  -e GIT_MED_FORGE_MEMORY_DB=true \
  -e GIT_MED_FORGE_AUTH0_ENABLED=false \
  --name git-med-forge \
  ghcr.io/qomraio/git-med-forge:latest

# Option B: Build and run from source
git clone https://github.com/QomraIo/git-med-forge.git
cd git-med-forge
cargo build --release
./target/release/git-med-forge --port 8080 --use-memory-db true --auth0-enabled false

# Verify server is up
curl http://localhost:8080/health
# git-med-forge ok
```

---

## Usage Examples

Once installed, invoke `git-med` directly from any directory:

```bash
# 1. Initialize a new medical dataset repository
git-med init my-dataset
cd my-dataset

# 2. Generate synthetic medical test data (e.g. DICOM series) in the current directory
git-med generate dicom . 2 1 3

# 3. Stage changes (parses medical metadata and routes binary payloads to LFS)
git-med add

# 4. Commit staged dataset
git-med commit -m "Initial medical dataset import"

# 5. Switch active imaging view (smart, dicom, brats)
git-med view dicom

# 6. Authenticate, create remote repository on server, add origin, and push
git-med login --server http://localhost:8080 --username alice --password secret
git-med remote create http://localhost:8080/alice/my-dataset.git
git-med remote add origin http://localhost:8080/alice/my-dataset.git
git-med push origin main
```

---

## Running Tests

```bash
# Run full unit and integration test suite (136 tests)
cargo test
```

---

## Releasing & Tagging (Maintainers)

Automated GitHub Actions pipelines test and build Ubuntu and macOS binaries on every release tag:

```bash
# 1. Create a version tag
git tag v0.1.0

# 2. Push tag to GitHub
git push origin v0.1.0
```
This triggers the CI pipeline to run tests, compile binaries, and publish a GitHub Release with `git-med-linux-x86_64.tar.gz`, `git-med-macos-arm64.tar.gz`, and a verified `SHA256SUMS` file.

---

## Documentation

* **Online Documentation (Astro Starlight)**: [https://qomraio.github.io/git-med/](https://qomraio.github.io/git-med/)
* **Local Docs**: Available in the [`docs/`](docs/README.md) directory.
  * [CLI Commands Reference](docs/commands.md): Details, usage syntax, and Git comparisons for all 15 commands.
  * [Authentication Guide](docs/authentication.md): Architecture, credential management, Auth0 integration, and access control.


