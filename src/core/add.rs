use regex::Regex;
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};

fn should_skip(path: &Path, skip: &[&str]) -> bool {
    for pattern in skip {
        if let Ok(re) = Regex::new(pattern) {
            if let Some(path_str) = path.to_str() {
                if re.is_match(path_str) {
                    return true;
                }
            }
        }
    }
    false
}

fn index(path: &Path) {
    print!("path to print is {0} \n", path.to_str().unwrap())
}

fn dfs() {
    let mut fringe: VecDeque<PathBuf> = VecDeque::new();
    fringe.push_back(PathBuf::from("."));

    // Read .gitignore and populate skip patterns
    let gitignore_content = fs::read_to_string(".gitignore").unwrap_or_default();
    let mut skip: Vec<&str> = gitignore_content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();

    // Always ignore git and git-med internal directories
    skip.push(r"\.git");
    skip.push(r"\.git-med");
    skip.push(r"(^|/)\.view");

    loop {
        let entries = fs::read_dir(fringe.pop_front().unwrap()).unwrap();

        for entry in entries {
            let entry = entry.unwrap();
            let file_type = entry.file_type().unwrap();
            let path = entry.path();

            if should_skip(&path, &skip) {
                continue;
            }

            if file_type.is_dir() {
                fringe.push_back(path);
            } else if file_type.is_file() {
                index(&path);
            }
        }
        if fringe.is_empty() {
            break;
        }
    }
}

pub fn add(_args: &Vec<String>) {
    print!("HI add");
    dfs();
}
