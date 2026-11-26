# git-med commit

## How to use
`git-med commit -m "<message>"`

## What
Persists the currently staged medical metadata tree into an immutable commit object in history and advances the current branch HEAD reference.

## Git similarity
| Aspect | git-med | Git | Key Differences |
| --- | --- | --- | --- |
| Syntax | `git-med commit -m "<message>"` | `git commit -m "<message>"` | Both accept `-m` or `--message` to record the commit description. |
| Hash Algorithm | BLAKE3 (64-character hexadecimal digest) | SHA-1 (40-hex) or SHA-256 (64-hex) | `git-med` utilizes BLAKE3 for cryptographic verification and maximum hashing performance on large data. |
| Commit Object Format | Custom text/binary object containing tree hash, parent commit hash, timestamp, and message | Git commit object containing tree, parents, author, committer, and message | `git-med` commits reference hierarchical medical attribute trees and LFS pointer hashes. |
| Message Requirement | Requires explicit `-m` / `--message` flag | Opens an interactive editor if message flag is omitted | `git-med` requires the message argument directly on the command line. |
