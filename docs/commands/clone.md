# git-med clone

## How to use
`git-med clone <repository_url> [destination_directory]`

## What
Clones a remote repository over HTTP/HTTPS from `git-med-forge`, downloading commits, metadata trees, and referenced binary medical assets into the local environment.

## Git similarity
| Aspect | git-med | Git | Key Differences |
| --- | --- | --- | --- |
| Command Syntax | `git-med clone <url> [destination]` | `git clone <url> [destination]` | Identical parameter structure and directory naming deduction. |
| Protocol Support | Smart HTTP / REST APIs over HTTP/HTTPS | Smart HTTP, SSH, Git native protocol | `git-med` communicates directly with `git-med-forge` REST endpoints and object stores. |
| LFS Hydration | Automatically fetches and verifies all referenced medical binary payloads into `.git-med/lfs/objects/` | Requires Git LFS filter hooks (`git lfs clone` / `git lfs pull`) | Large file synchronization is built directly into core cloning without external plugins. |
| Authentication | Automatically checks `GIT_MED_TOKEN` or `~/.git-med/credentials` for matching server origin | Uses Git credential helpers, SSH keys, or `.netrc` | Direct origin-matched Bearer token injection. |
