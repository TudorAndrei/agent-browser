use super::Step;
use crate::connection::get_socket_dir;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Metadata {
    pub title: String,
    pub last_url: Option<String>,
}

pub fn create(session_id: &str) -> Result<PathBuf, String> {
    let dir = get_socket_dir();
    fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create codegen sidecar directory: {e}"))?;
    let path = dir.join(format!("{}.codegen.jsonl", session_id));
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .map_err(|e| format!("Failed to create codegen sidecar: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("Failed to secure codegen sidecar: {e}"))?;
    }
    Ok(path)
}

fn metadata_path(path: &Path) -> PathBuf {
    path.with_extension("codegen.meta.json")
}

pub fn write_metadata(path: &Path, metadata: &Metadata) -> Result<(), String> {
    let path = metadata_path(path);
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .map_err(|e| format!("Failed to create codegen metadata: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("Failed to secure codegen metadata: {e}"))?;
    }
    serde_json::to_writer(file, metadata)
        .map_err(|e| format!("Failed to write codegen metadata: {e}"))
}

pub fn read_metadata(session_id: &str) -> Result<Option<(PathBuf, Metadata)>, String> {
    let path = get_socket_dir().join(format!("{session_id}.codegen.jsonl"));
    let metadata_path = metadata_path(&path);
    if !metadata_path.exists() {
        return Ok(None);
    }
    let file = fs::File::open(&metadata_path)
        .map_err(|e| format!("Failed to read codegen metadata: {e}"))?;
    let metadata =
        serde_json::from_reader(file).map_err(|e| format!("Invalid codegen metadata: {e}"))?;
    Ok(Some((path, metadata)))
}

pub fn remove_metadata(path: &Path) {
    let _ = fs::remove_file(metadata_path(path));
}

pub fn append(path: &Path, step: &Step) -> Result<(), String> {
    let line = serde_json::to_string(step).map_err(|e| e.to_string())?;
    let mut file = OpenOptions::new()
        .append(true)
        .open(path)
        .map_err(|e| format!("Failed to open codegen sidecar: {e}"))?;
    writeln!(file, "{line}").map_err(|e| format!("Failed to append codegen step: {e}"))
}

pub fn read(path: &Path) -> Result<Vec<Step>, String> {
    let content =
        fs::read_to_string(path).map_err(|e| format!("Failed to read codegen sidecar: {e}"))?;
    content
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).map_err(|e| format!("Invalid codegen sidecar: {e}")))
        .collect()
}

pub fn rewrite(path: &Path, steps: &[Step]) -> Result<(), String> {
    let mut output = String::new();
    for step in steps {
        output.push_str(&serde_json::to_string(step).map_err(|e| e.to_string())?);
        output.push('\n');
    }
    fs::write(path, output).map_err(|e| format!("Failed to rewrite codegen sidecar: {e}"))
}
