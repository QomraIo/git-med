use std::fs::{self, File};
use std::path::Path;

pub fn init(args: &Vec<String>) {
    if args.len() <= 2 {
        panic!("Please provide a name");
    }
    if Path::new(".git-med").exists() {
        panic!(".git-med already exists");
    }
    init_at(Path::new(&args[2]));
}

/// Initialize a `.git-med` repository directory under `repo_path`.
pub(crate) fn init_at(repo_path: &Path) {
    let git_med_path = repo_path.join(".git-med");

    if git_med_path.exists() {
        panic!(".git-med already exists");
    }

    let dirs = [
        git_med_path.join("refs/heads"),
        git_med_path.join("refs/remotes/origin"),
        git_med_path.join("refs/tags"),
        git_med_path.join("objects"),
    ];

    for dir in &dirs {
        fs::create_dir_all(dir).expect("Failed to create directory");
    }

    File::create(git_med_path.join("index")).expect("Failed to create index file");
    fs::write(git_med_path.join("HEAD"), "ref: refs/heads/main\n")
        .expect("Failed to create HEAD file");
    fs::write(git_med_path.join("VIEW"), "QOMRA\n").expect("Failed to create VIEW file");
    fs::write(
        git_med_path.join("lfs_track"),
        crate::files::lfs::DEFAULT_LFS_PATTERNS,
    )
    .expect("Failed to create lfs_track file");
    File::create(git_med_path.join("refs/heads/main"))
        .expect("Failed to create refs/heads/main file");
}
