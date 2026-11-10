//! Helpers shared by the tests.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A directory removed again when dropped.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn tempdir() -> TempDir {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "asterofix-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    TempDir(dir)
}

pub fn write(path: &Path, data: impl AsRef<[u8]>) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, data).unwrap();
}

/// Creates a SteamFix game folder; `real_id` 0 leaves out the ini.
pub fn add_game(lib: &Path, folder: &str, real_id: u32) -> PathBuf {
    let dir = lib.join(folder);
    write(&dir.join(format!("{folder}.exe")), "x");
    write(&dir.join("SteamFix64.dll"), "x");
    if real_id != 0 {
        write(&dir.join("SteamFix.ini"), format!("[Main]\nRealAppId={real_id}\nFakeAppId=480\n"));
    }
    dir
}

/// A stand-in for an Inno Setup installer: just the marker that tells it apart.
pub fn inno_installer(path: &Path) -> PathBuf {
    write(path, b"MZ\0\0Inno Setup Setup Data (5.5.0) (u)");
    path.to_path_buf()
}

/// A native Steam in `home` with Valve's Proton builds in `protons` (folder names, "Proton 9.0")
/// and `default` (Steam's internal name, "proton_9") its default tool. Returns its root.
pub fn fake_steam(home: &Path, protons: &[&str], default: &str) -> PathBuf {
    let root = home.join(".local/share/Steam");
    std::fs::create_dir_all(root.join("steamapps")).unwrap();
    for p in protons {
        write(&root.join("steamapps/common").join(p).join("proton"), "");
    }
    set_default_proton(&root, default);
    root
}

/// Makes `name` (Steam's internal name) Steam's default tool.
pub fn set_default_proton(root: &Path, name: &str) {
    write(
        &root.join("config/config.vdf"),
        format!(
            r#""InstallConfigStore" {{ "Software" {{ "Valve" {{ "Steam" {{ "CompatToolMapping" {{ "0" {{ "name" "{name}" }} }} }} }} }} }}"#
        ),
    );
}

/// A custom Proton build in Steam's compatibilitytools.d, as GE-Proton installs.
pub fn add_custom_proton(root: &Path, name: &str) {
    let dir = root.join("compatibilitytools.d").join(name);
    write(&dir.join("proton"), "");
    write(
        &dir.join("compatibilitytool.vdf"),
        format!(
            r#""compatibilitytools" {{ "compat_tools" {{ "{name}" {{ "install_path" "." "display_name" "{name}" }} }} }}"#
        ),
    );
}
