mod import_jobs;
mod path_policy;
mod protocol;
mod tool_schema;

use super::*;
use std::path::Path;

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn clear_root_dir_env() {
    std::env::remove_var(ROOT_DIR_ENV);
    std::env::remove_var(ROOT_DIR_ALLOWED_ROOTS_ENV);
    std::env::remove_var(IMPORT_ALLOWED_ROOTS_ENV);
    std::env::remove_var(LOCAL_PATH_DISCLOSURE_ENV);
}

fn set_allowed_roots(paths: &[&Path]) {
    let joined = std::env::join_paths(paths).expect("join allowed roots");
    std::env::set_var(ROOT_DIR_ALLOWED_ROOTS_ENV, joined);
}

fn set_allowed_import_roots(paths: &[&Path]) {
    let joined = std::env::join_paths(paths).expect("join allowed import roots");
    std::env::set_var(IMPORT_ALLOWED_ROOTS_ENV, joined);
}

fn canonical_path_string(path: &Path) -> String {
    path.canonicalize()
        .expect("canonical path")
        .into_os_string()
        .into_string()
        .expect("utf-8 canonical path")
}
