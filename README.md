# git-med - pre-alpha 

`git-med` is an experimental/evolving distributed version control system (DVCS). We use it to power Qomra Datasets. We are using this project to experiment with ideas. 

## Ideas we are exploring:
- Import data from any format and interface it in any format
- Versioning for AI and human workflows 
- Switch between diffrent standerds like DICOM, BIDS ...etc.
- Lazy download (VFS, FUSE, NFS, ...)
- Compression (xet, atlas based compression,...)
- Fast, secure, scalable and open. 

In current version we opted for building everything thing from scratch, make the architecture modular, write parts we need to understand deeply by hand and prototype the rest with LLMs. The goal is to evolve this system to power future PACS/VNA (Vendor Neutral Archive) systems and AI research tools.

We are considering transforming this to be a git extension/plugin or at least compatible with git.

Mainly, Antigravity with Gemini 3.8 High was used. Claude Opus 4.6 was occasionally for review.

---

## Getting Started

### Prerequisites
- [Rust & Cargo](https://www.rust-lang.org/tools/install) (Rust 1.85+ with 2024 edition support)

### Build Commands

```bash
# Build debug binary (located in ./target/debug/git-med)
cargo build

# Build optimized release binary (located in ./target/release/git-med)
cargo build --release

# Install binary to ~/.cargo/bin (enables `git-med` globally on PATH)
cargo install --path .
```

### Run Commands

#### Running via Cargo
Pass arguments to `git-med` by placing them after `--`:
```bash
# General syntax
cargo run -- <command> [arguments...]

# Examples:
# Initialize a new repository
cargo run -- init my-dataset

# Generate synthetic medical test data (e.g. DICOM series)
cargo run -- generate dicom ./my-dataset 2 1 3

# Stage changes according to active medical view
cargo run -- add

# Commit staged changes
cargo run -- commit -m "Initial medical dataset commit"

# Switch or query dataset view (qomra, dicom, brats)
cargo run -- view dicom
```

#### Running the Compiled Binary Directly
```bash
# Run debug binary
./target/debug/git-med <command> [arguments...]

# Run release binary
./target/release/git-med <command> [arguments...]

# Run installed CLI (when ~/.cargo/bin is in PATH)
git-med <command> [arguments...]
```

### Running Tests

```bash
# Run unit and integration tests
cargo test
```

---

## Documentation

Full documentation is available in the [`docs/`](docs/README.md) directory:
* [CLI Commands Reference](docs/commands.md): Details, usage syntax, and Git comparisons for all 15 commands.
* [Authentication Guide](docs/authentication.md): Architecture, credential management, Auth0 integration, and access control.


