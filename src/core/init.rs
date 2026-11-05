use std::fs::{self, File};
use std::path::Path;

pub fn init(args: &Vec<String>) {
    if args.len() <= 2 {
        panic!("Please provide a name");
    }
    let name = &args[2];
    let repo_path = Path::new(name);
    let git_med_path = repo_path.join(".git-med");

    if Path::new(".git-med").exists() || git_med_path.exists() {
        panic!(".git-med already exists");
    }

    let dirs = [
        git_med_path.join("refs/heads"),
        git_med_path.join("refs/remotes/origin"),
        git_med_path.join("refs/tags"),
        git_med_path.join("objects"),
        git_med_path.join("meta_objects"),
    ];

    for dir in &dirs {
        fs::create_dir_all(dir).expect("Failed to create directory");
    }

    File::create(git_med_path.join("index")).expect("Failed to create index file");
    fs::write(git_med_path.join("HEAD"), "ref: refs/heads/main\n")
        .expect("Failed to create HEAD file");
    File::create(git_med_path.join("VIEW")).expect("Failed to create index file");
    fs::write(git_med_path.join("VIEW"), "DICOM\n").expect("Failed to create HEAD file");
    File::create(git_med_path.join("refs/heads/main"))
        .expect("Failed to create refs/heads/main file");
}
