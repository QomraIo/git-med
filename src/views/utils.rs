use std::path::Path;

pub fn get_name(opt_path: &Option<&Path>) -> String {
    opt_path
        .and_then(|p| p.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap()
}
