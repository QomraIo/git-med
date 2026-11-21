# git-med init

## How to use
`git-med init <repository_name>`

## What
Initializes a new `git-med` repository by creating the `.git-med` directory structure (branches, object store, staging index, and HEAD pointer) configured for versioning medical datasets.

## Git similarity
| Aspect | git-med | Git | Key Differences |
| --- | --- | --- | --- |
| Directory Created | `.git-med/` | `.git/` | `git-med` isolates metadata in `.git-med/` to avoid collision with standard Git repositories. |
| Command Argument | `git-med init <repository_name>` | `git init [directory]` | `git-med init` requires a directory name argument and initializes the repository inside that folder. |
| Initial Branch | `refs/heads/main` | `refs/heads/main` (or `master`) | Both point `HEAD` to the default `main` branch. |
| Initial Structure | Creates `refs/heads`, `refs/tags`, `refs/remotes/origin`, `objects`, `index` | Creates `refs/`, `objects/`, `hooks/`, `info/`, `config`, `HEAD` | `git-med` prepares storage paths specifically tailored for medical dataset graphs and LFS blobs. |
