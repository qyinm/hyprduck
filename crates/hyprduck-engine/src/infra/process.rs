use std::path::{Path, PathBuf};

pub(crate) fn resolve_binary(name: &str, common_paths: &[&str]) -> PathBuf {
    if let Some(path) = find_binary_on_path(name) {
        return path;
    }

    common_paths
        .iter()
        .map(PathBuf::from)
        .find(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from(name))
}

fn find_binary_on_path(name: &str) -> Option<PathBuf> {
    if Path::new(name).components().count() > 1 {
        let path = PathBuf::from(name);
        return path.exists().then_some(path);
    }

    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.exists())
    })
}
