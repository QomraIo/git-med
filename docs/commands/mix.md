# git-med mix

## How to use
`git-med mix <result_branch> <spec1> <spec2> [--left|--right] [-m "<message>"]`

## What
Combines and synthesizes medical dataset cohorts across multiple branches, local repositories, or remote URLs into a new unified target branch with zero-copy binary linking.

## Git similarity
| Aspect | git-med | Git | Key Differences |
| --- | --- | --- | --- |
| Core Concept | Multi-repository cohort synthesis and dataset merging | Submodules, subtree merges, or cross-repo cherry-picking | Git has no equivalent command. Git manages individual isolated repositories; `git-med mix` enables declarative dataset blending across multiple repos. |
| Storage Efficiency | Zero-copy local LFS linking (hardlinks/symlinks raw imaging blobs across repositories) | Cloning or copying files physically across repositories | Eliminates disk duplication when combining multi-gigabyte imaging cohorts. |
| Specification Syntax | Accepts specifiers such as `self::<branch>`, `<local_path>::<branch>`, or `<remote_url>::<branch>` | Requires adding distinct remotes, fetching objects, and complex ref merging | Single CLI command blends cohorts directly from diverse sources. |
| Conflict Resolution | Supports `--left` and `--right` conflict breaker options | Strategy options (`ours`/`theirs`) across separate operations | Integrated tie-breaking across multi-source cohort compositions. |
