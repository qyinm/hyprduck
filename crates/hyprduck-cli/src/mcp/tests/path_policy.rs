use super::super::*;
use super::*;

#[test]
fn validate_import_source_path_accepts_file_inside_allowed_root() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    clear_root_dir_env();
    let allowed = tempfile::tempdir().expect("allowed dir");
    let source = allowed.path().join("source.md");
    std::fs::write(&source, "# Source\n").expect("source file");

    set_allowed_import_roots(&[allowed.path()]);
    let validated =
        validate_import_source_path(&source.display().to_string()).expect("valid source path");

    assert_eq!(validated, source.canonicalize().expect("canonical source"));
    clear_root_dir_env();
}

#[test]
fn validate_import_source_path_rejects_file_outside_allowed_root() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    clear_root_dir_env();
    let allowed = tempfile::tempdir().expect("allowed dir");
    let outside = tempfile::tempdir().expect("outside dir");
    let source = outside.path().join("source.md");
    std::fs::write(&source, "# Source\n").expect("source file");

    set_allowed_import_roots(&[allowed.path()]);
    let error = validate_import_source_path(&source.display().to_string())
        .expect_err("outside source rejected");

    assert!(error
        .to_string()
        .contains("HYPRDUCK_MCP_ALLOWED_IMPORT_ROOTS"));
    clear_root_dir_env();
}

#[test]
fn validate_import_source_path_rejects_directory() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    clear_root_dir_env();
    let allowed = tempfile::tempdir().expect("allowed dir");

    set_allowed_import_roots(&[allowed.path()]);
    let error = validate_import_source_path(&allowed.path().display().to_string())
        .expect_err("directory source rejected");

    assert!(error.to_string().contains("regular file"));
    clear_root_dir_env();
}

#[test]
fn validate_import_source_path_rejects_file_as_allowed_root() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    clear_root_dir_env();
    let allowed = tempfile::tempdir().expect("allowed dir");
    let source = allowed.path().join("source.md");
    std::fs::write(&source, "# Source\n").expect("source file");

    set_allowed_import_roots(&[source.as_path()]);
    let error =
        validate_import_source_path(&source.display().to_string()).expect_err("file root rejected");

    assert!(error.to_string().contains("must be a directory"));
    clear_root_dir_env();
}

#[test]
#[cfg(unix)]
fn validate_import_source_path_rejects_symlink_escape() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    clear_root_dir_env();
    let temp = tempfile::tempdir().expect("temp dir");
    let allowed = temp.path().join("allowed");
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&allowed).expect("allowed dir");
    std::fs::create_dir_all(&outside).expect("outside dir");
    let outside_file = outside.join("source.md");
    let symlink = allowed.join("linked.md");
    std::fs::write(&outside_file, "# Source\n").expect("outside source");
    std::os::unix::fs::symlink(&outside_file, &symlink).expect("symlink");

    set_allowed_import_roots(&[allowed.as_path()]);
    let error = validate_import_source_path(&symlink.display().to_string())
        .expect_err("symlink escape rejected");

    assert!(error
        .to_string()
        .contains("HYPRDUCK_MCP_ALLOWED_IMPORT_ROOTS"));
    clear_root_dir_env();
}

#[test]
fn import_document_format_infers_pdf() {
    assert_eq!(
        import_document_format(Path::new("source.pdf"), None).expect("pdf format"),
        DocumentFormat::Pdf
    );
}

#[test]
fn import_document_format_infers_markdown() {
    assert_eq!(
        import_document_format(Path::new("source.md"), None).expect("markdown format"),
        DocumentFormat::Markdown
    );
    assert_eq!(
        import_document_format(Path::new("source.markdown"), None).expect("markdown format"),
        DocumentFormat::Markdown
    );
}

#[test]
fn import_document_format_infers_office_and_image_formats() {
    assert_eq!(
        import_document_format(Path::new("source.docx"), None).expect("docx format"),
        DocumentFormat::Docx
    );
    assert_eq!(
        import_document_format(Path::new("source.doc"), None).expect("doc format"),
        DocumentFormat::Doc
    );
    assert_eq!(
        import_document_format(Path::new("source.png"), None).expect("image format"),
        DocumentFormat::Image
    );
}

#[test]
fn import_document_format_uses_explicit_format() {
    assert_eq!(
        import_document_format(Path::new("source.txt"), Some("IMAGE".into()))
            .expect("explicit image format"),
        DocumentFormat::Image
    );
}

#[test]
fn import_document_format_rejects_unknown_extension() {
    let error = import_document_format(Path::new("source.txt"), None)
        .expect_err("unknown extension rejected");
    assert!(error.to_string().contains("unsupported import format"));
}

#[test]
fn read_scope_rejects_root_dir_without_dev_env() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    clear_root_dir_env();
    let mut arguments = Map::new();
    arguments.insert("rootDir".into(), Value::String("/tmp/hyprduck-test".into()));

    let error = read_scope(&arguments).expect_err("rootDir should be disabled by default");
    assert!(error.to_string().contains("rootDir is disabled"));
    clear_root_dir_env();
}

#[test]
fn read_scope_rejects_root_dir_when_dev_env_is_not_one() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    clear_root_dir_env();
    let mut arguments = Map::new();
    arguments.insert("rootDir".into(), Value::String("/tmp/hyprduck-test".into()));

    std::env::set_var(ROOT_DIR_ENV, "0");
    let zero_error = read_scope(&arguments).expect_err("rootDir=0 should stay disabled");
    assert!(zero_error.to_string().contains("rootDir is disabled"));

    std::env::set_var(ROOT_DIR_ENV, "");
    let empty_error = read_scope(&arguments).expect_err("empty rootDir env should stay disabled");
    assert!(empty_error.to_string().contains("rootDir is disabled"));

    clear_root_dir_env();
}

#[test]
fn read_scope_rejects_root_dir_without_allowed_roots() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    clear_root_dir_env();
    let temp = tempfile::tempdir().expect("temp dir");
    let mut arguments = Map::new();
    arguments.insert(
        "rootDir".into(),
        Value::String(temp.path().display().to_string()),
    );

    std::env::set_var(ROOT_DIR_ENV, "1");
    let error = read_scope(&arguments).expect_err("allowlist should be required");
    assert!(error.to_string().contains("HYPRDUCK_MCP_ALLOWED_ROOTS"));
    clear_root_dir_env();
}

#[test]
fn read_scope_accepts_allowlisted_root_dir() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    clear_root_dir_env();
    let temp = tempfile::tempdir().expect("temp dir");
    let mut arguments = Map::new();
    arguments.insert(
        "rootDir".into(),
        Value::String(temp.path().display().to_string()),
    );

    std::env::set_var(ROOT_DIR_ENV, "1");
    set_allowed_roots(&[temp.path()]);
    let scope = read_scope(&arguments).expect("allowlisted rootDir");
    let expected_root_dir = canonical_path_string(temp.path());
    assert_eq!(scope.root_dir.as_deref(), Some(expected_root_dir.as_str()));
    clear_root_dir_env();
}

#[test]
#[cfg(unix)]
fn read_scope_stores_canonical_root_dir() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    clear_root_dir_env();
    let temp = tempfile::tempdir().expect("temp dir");
    let actual = temp.path().join("actual");
    let symlink = temp.path().join("linked-root");
    std::fs::create_dir_all(&actual).expect("actual dir");
    std::os::unix::fs::symlink(&actual, &symlink).expect("symlink");
    let mut arguments = Map::new();
    arguments.insert(
        "rootDir".into(),
        Value::String(symlink.display().to_string()),
    );

    std::env::set_var(ROOT_DIR_ENV, "1");
    set_allowed_roots(&[actual.as_path()]);
    let scope = read_scope(&arguments).expect("allowlisted symlink rootDir");
    let expected_root_dir = canonical_path_string(actual.as_path());
    assert_eq!(scope.root_dir.as_deref(), Some(expected_root_dir.as_str()));
    clear_root_dir_env();
}

#[test]
fn read_scope_rejects_root_dir_outside_allowed_roots() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    clear_root_dir_env();
    let allowed = tempfile::tempdir().expect("allowed dir");
    let outside = tempfile::tempdir().expect("outside dir");
    let mut arguments = Map::new();
    arguments.insert(
        "rootDir".into(),
        Value::String(outside.path().display().to_string()),
    );

    std::env::set_var(ROOT_DIR_ENV, "1");
    set_allowed_roots(&[allowed.path()]);
    let error = read_scope(&arguments).expect_err("outside rootDir rejected");
    assert!(error.to_string().contains("HYPRDUCK_MCP_ALLOWED_ROOTS"));
    clear_root_dir_env();
}

#[test]
#[cfg(unix)]
fn read_scope_rejects_symlinked_root_dir_outside_allowed_roots() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    clear_root_dir_env();
    let temp = tempfile::tempdir().expect("temp dir");
    let allowed = temp.path().join("allowed");
    let outside = temp.path().join("outside");
    let symlink = temp.path().join("linked-root");
    std::fs::create_dir_all(&allowed).expect("allowed dir");
    std::fs::create_dir_all(&outside).expect("outside dir");
    std::os::unix::fs::symlink(&outside, &symlink).expect("symlink");
    let mut arguments = Map::new();
    arguments.insert(
        "rootDir".into(),
        Value::String(symlink.display().to_string()),
    );

    std::env::set_var(ROOT_DIR_ENV, "1");
    set_allowed_roots(&[allowed.as_path()]);
    let error = read_scope(&arguments).expect_err("symlink escape rejected");
    assert!(error.to_string().contains("HYPRDUCK_MCP_ALLOWED_ROOTS"));
    clear_root_dir_env();
}

#[test]
fn resource_uri_rejects_root_dir_without_dev_env() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    clear_root_dir_env();

    let error = parse_resource_uri("hyprduck://brain/default/wiki/index.md?rootDir=/tmp")
        .expect_err("resource rootDir should be disabled by default");
    assert!(error.to_string().contains("rootDir is disabled"));
    clear_root_dir_env();
}

#[test]
fn resource_uri_rejects_root_dir_when_dev_env_is_not_one() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    clear_root_dir_env();
    std::env::set_var(ROOT_DIR_ENV, "0");
    let zero_error = parse_resource_uri("hyprduck://brain/default/wiki/index.md?rootDir=/tmp")
        .expect_err("rootDir=0 should stay disabled for resources");
    assert!(zero_error.to_string().contains("rootDir is disabled"));

    std::env::set_var(ROOT_DIR_ENV, "");
    let empty_error = parse_resource_uri("hyprduck://brain/default/wiki/index.md?rootDir=/tmp")
        .expect_err("empty rootDir env should stay disabled for resources");
    assert!(empty_error.to_string().contains("rootDir is disabled"));

    clear_root_dir_env();
}

#[test]
fn resource_uri_accepts_allowlisted_root_dir() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    clear_root_dir_env();
    let temp = tempfile::tempdir().expect("temp dir");
    let uri = format!(
        "hyprduck://brain/default/wiki/index.md?rootDir={}",
        temp.path().display()
    );

    std::env::set_var(ROOT_DIR_ENV, "1");
    set_allowed_roots(&[temp.path()]);
    let resource = parse_resource_uri(&uri).expect("allowlisted resource rootDir");
    let expected_root_dir = canonical_path_string(temp.path());
    assert_eq!(
        resource.scope.root_dir.as_deref(),
        Some(expected_root_dir.as_str())
    );
    clear_root_dir_env();
}

#[test]
fn resource_uri_rejects_root_dir_outside_allowed_roots() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    clear_root_dir_env();
    let allowed = tempfile::tempdir().expect("allowed dir");
    let outside = tempfile::tempdir().expect("outside dir");
    let uri = format!(
        "hyprduck://brain/default/wiki/index.md?rootDir={}",
        outside.path().display()
    );

    std::env::set_var(ROOT_DIR_ENV, "1");
    set_allowed_roots(&[allowed.path()]);
    let error = parse_resource_uri(&uri).expect_err("outside resource rootDir rejected");
    assert!(error.to_string().contains("HYPRDUCK_MCP_ALLOWED_ROOTS"));
    clear_root_dir_env();
}

#[test]
#[cfg(unix)]
fn resource_uri_rejects_symlinked_root_dir_outside_allowed_roots() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    clear_root_dir_env();
    let temp = tempfile::tempdir().expect("temp dir");
    let allowed = temp.path().join("allowed");
    let outside = temp.path().join("outside");
    let symlink = temp.path().join("linked-root");
    std::fs::create_dir_all(&allowed).expect("allowed dir");
    std::fs::create_dir_all(&outside).expect("outside dir");
    std::os::unix::fs::symlink(&outside, &symlink).expect("symlink");
    let uri = format!(
        "hyprduck://brain/default/wiki/index.md?rootDir={}",
        symlink.display()
    );

    std::env::set_var(ROOT_DIR_ENV, "1");
    set_allowed_roots(&[allowed.as_path()]);
    let error = parse_resource_uri(&uri).expect_err("symlink escape resource rootDir rejected");
    assert!(error.to_string().contains("HYPRDUCK_MCP_ALLOWED_ROOTS"));
    clear_root_dir_env();
}

#[test]
#[cfg(unix)]
fn resource_uri_stores_canonical_root_dir() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    clear_root_dir_env();
    let temp = tempfile::tempdir().expect("temp dir");
    let actual = temp.path().join("actual");
    let symlink = temp.path().join("linked-root");
    std::fs::create_dir_all(&actual).expect("actual dir");
    std::os::unix::fs::symlink(&actual, &symlink).expect("symlink");
    let uri = format!(
        "hyprduck://brain/default/wiki/index.md?rootDir={}",
        symlink.display()
    );

    std::env::set_var(ROOT_DIR_ENV, "1");
    set_allowed_roots(&[actual.as_path()]);
    let resource = parse_resource_uri(&uri).expect("allowlisted resource rootDir");
    let expected_root_dir = canonical_path_string(actual.as_path());
    assert_eq!(
        resource.scope.root_dir.as_deref(),
        Some(expected_root_dir.as_str())
    );
    clear_root_dir_env();
}
