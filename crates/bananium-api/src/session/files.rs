//! A file manager over an instance's game directory, so a frontend can
//! browse, edit, create, rename, import and delete files there without
//! touching the filesystem itself.
//!
//! Every path a frontend passes is *relative to the game directory* and
//! `/`-separated. [`Session::resolve_game_path`] is the single gate that
//! turns one into a real path: absolute paths, drive prefixes and `..`/`.`
//! components are refused outright rather than normalised, so no request
//! can reach outside the instance.

use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use super::Session;
use crate::error::{Error, Result};
use crate::output::{CommandOutput, FileEntry};

/// Largest file `FileRead` returns for in-app editing. Bigger files (world
/// data, logs from a long session) are for an external editor.
const MAX_EDIT_BYTES: u64 = 2 * 1024 * 1024;

impl Session {
    /// The real path for `rel` inside `instance`'s game directory. An empty
    /// `rel` is the game directory itself.
    fn resolve_game_path(&self, instance: &str, rel: &str) -> Result<PathBuf> {
        self.instances().resolve(Some(instance))?;
        join_relative(&self.paths.instance_minecraft_dir(instance), rel)
            .ok_or_else(|| Error::InvalidPath(rel.to_string()))
    }

    /// Like [`Session::resolve_game_path`], but refuses the game directory
    /// itself — for operations that must never apply to the root.
    fn resolve_game_child(&self, instance: &str, rel: &str) -> Result<PathBuf> {
        if rel.split(['/', '\\']).all(|p| p.is_empty()) {
            return Err(Error::InvalidPath(rel.to_string()));
        }
        self.resolve_game_path(instance, rel)
    }

    /// `Command::FileList`: the entries of one directory, folders first.
    pub(super) fn file_list(&self, instance: &str, dir: &str) -> Result<CommandOutput> {
        let root = self.resolve_game_path(instance, dir)?;
        // A fresh instance may not have launched yet, so its game directory
        // can be missing; show it as empty rather than an error.
        if dir.trim_matches(['/', '\\']).is_empty() {
            std::fs::create_dir_all(&root)?;
        }
        let prefix = dir.trim_matches(['/', '\\']).replace('\\', "/");
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&root)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let meta = entry.metadata()?;
            entries.push(FileEntry {
                path: if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}/{name}")
                },
                name,
                is_dir: meta.is_dir(),
                size: if meta.is_dir() { 0 } else { meta.len() },
                modified_unix: meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or_default(),
                abs_path: entry.path(),
            });
        }
        entries.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(CommandOutput::FileListed {
            instance: instance.to_string(),
            path: prefix,
            entries,
        })
    }

    /// `Command::FileRead`: a text file's contents, for in-app editing.
    pub(super) fn file_read(&self, instance: &str, rel: &str) -> Result<CommandOutput> {
        let path = self.resolve_game_child(instance, rel)?;
        if std::fs::metadata(&path)?.len() > MAX_EDIT_BYTES {
            return Err(Error::FileTooLarge(rel.to_string()));
        }
        let bytes = std::fs::read(&path)?;
        let text = String::from_utf8(bytes).map_err(|_| Error::NotText(rel.to_string()))?;
        Ok(CommandOutput::FileContents {
            path: rel.to_string(),
            text,
        })
    }

    /// `Command::FileWrite`: save `text` to a file, creating it (and any
    /// missing parent folders) if needed. With `create_new`, an existing
    /// file is an error instead of being overwritten.
    pub(super) fn file_write(
        &self,
        instance: &str,
        rel: &str,
        text: &str,
        create_new: bool,
    ) -> Result<CommandOutput> {
        let path = self.resolve_game_child(instance, rel)?;
        if create_new && path.exists() {
            return Err(Error::FileExists(rel.to_string()));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, text)?;
        Ok(CommandOutput::FileWritten {
            path: rel.to_string(),
        })
    }

    /// `Command::FileCreateDir`.
    pub(super) fn file_create_dir(&self, instance: &str, rel: &str) -> Result<CommandOutput> {
        let path = self.resolve_game_child(instance, rel)?;
        if path.exists() {
            return Err(Error::FileExists(rel.to_string()));
        }
        std::fs::create_dir_all(&path)?;
        Ok(CommandOutput::FileWritten {
            path: rel.to_string(),
        })
    }

    /// `Command::FileRename`: rename or move within the game directory.
    pub(super) fn file_rename(
        &self,
        instance: &str,
        from: &str,
        to: &str,
    ) -> Result<CommandOutput> {
        let source = self.resolve_game_child(instance, from)?;
        let dest = self.resolve_game_child(instance, to)?;
        if dest.exists() {
            return Err(Error::FileExists(to.to_string()));
        }
        std::fs::rename(&source, &dest)?;
        Ok(CommandOutput::FileWritten {
            path: to.to_string(),
        })
    }

    /// `Command::FileDelete`: a file, or a folder and everything in it.
    pub(super) fn file_delete(&self, instance: &str, rel: &str) -> Result<CommandOutput> {
        let path = self.resolve_game_child(instance, rel)?;
        // `symlink_metadata` so a link is removed itself, never followed.
        if std::fs::symlink_metadata(&path)?.is_dir() {
            std::fs::remove_dir_all(&path)?;
        } else {
            std::fs::remove_file(&path)?;
        }
        Ok(CommandOutput::FileDeleted {
            path: rel.to_string(),
        })
    }

    /// `Command::FileImport`: copy files or folders from anywhere on disk
    /// into a directory of the game directory. Existing names are refused
    /// rather than overwritten.
    pub(super) fn file_import(
        &self,
        instance: &str,
        dir: &str,
        sources: &[PathBuf],
    ) -> Result<CommandOutput> {
        let dest_dir = self.resolve_game_path(instance, dir)?;
        std::fs::create_dir_all(&dest_dir)?;
        for source in sources {
            let name = source
                .file_name()
                .ok_or_else(|| Error::InvalidPath(source.display().to_string()))?;
            let dest = dest_dir.join(name);
            if dest.exists() {
                return Err(Error::FileExists(name.to_string_lossy().into_owned()));
            }
            copy_recursive(source, &dest)?;
        }
        Ok(CommandOutput::FileImported {
            count: sources.len() as u32,
        })
    }
}

/// `root` joined with the `/`- or `\`-separated relative path `rel`, or
/// `None` if any component could leave `root` (`..`, `.`, a drive prefix or
/// root). Empty components are skipped, so a leading `/` stays relative.
/// Shared with modpack installs, whose file paths come from the pack.
pub(super) fn join_relative(root: &Path, rel: &str) -> Option<PathBuf> {
    let mut path = root.to_path_buf();
    for part in rel.split(['/', '\\']).filter(|p| !p.is_empty()) {
        let mut components = Path::new(part).components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(name)), None) => path.push(name),
            _ => return None,
        }
    }
    Some(path)
}

fn copy_recursive(from: &Path, to: &Path) -> Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &to.join(entry.file_name()))?;
        }
    } else {
        std::fs::copy(from, to)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use bananium_core::{Config, Paths};
    use bananium_instance::InstanceStore;

    use super::*;

    fn session() -> (tempfile::TempDir, Session) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        InstanceStore::new(paths.clone())
            .create_named("1.21.1", Some("main"))
            .unwrap();
        let session = Session::new(paths, Config::default()).unwrap();
        (dir, session)
    }

    #[test]
    fn paths_outside_the_game_directory_are_refused() {
        let (_dir, s) = session();
        for bad in ["..", "../instance.toml", "config/../../x", "./x"] {
            assert!(
                matches!(s.resolve_game_path("main", bad), Err(Error::InvalidPath(_))),
                "{bad} should be refused"
            );
        }
        #[cfg(windows)]
        assert!(matches!(
            s.resolve_game_path("main", "C:/Windows"),
            Err(Error::InvalidPath(_))
        ));
        // A leading separator doesn't make a path absolute here: it's split
        // into plain components and still lands inside the game directory.
        let game_dir = s.paths.instance_minecraft_dir("main");
        assert!(s
            .resolve_game_path("main", "/etc/passwd")
            .unwrap()
            .starts_with(&game_dir));
        assert!(s.resolve_game_path("main", "config/sodium.json").is_ok());
        assert!(matches!(
            s.file_delete("main", ""),
            Err(Error::InvalidPath(_))
        ));
    }

    #[test]
    fn write_list_read_rename_delete_round_trip() {
        let (_dir, s) = session();
        s.file_write("main", "config/a.txt", "hello", true).unwrap();
        assert!(matches!(
            s.file_write("main", "config/a.txt", "again", true),
            Err(Error::FileExists(_))
        ));
        s.file_create_dir("main", "config/sub").unwrap();

        let CommandOutput::FileListed { entries, path, .. } =
            s.file_list("main", "config").unwrap()
        else {
            panic!("wrong output")
        };
        assert_eq!(path, "config");
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["sub", "a.txt"], "folders first");
        assert_eq!(entries[1].path, "config/a.txt");

        let CommandOutput::FileContents { text, .. } = s.file_read("main", "config/a.txt").unwrap()
        else {
            panic!("wrong output")
        };
        assert_eq!(text, "hello");

        s.file_rename("main", "config/a.txt", "config/sub/b.txt")
            .unwrap();
        s.file_delete("main", "config").unwrap();
        let CommandOutput::FileListed { entries, .. } = s.file_list("main", "").unwrap() else {
            panic!("wrong output")
        };
        assert!(entries.is_empty());
    }

    #[test]
    fn binary_files_are_not_opened_as_text() {
        let (_dir, s) = session();
        let path = s.resolve_game_path("main", "level.dat").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, [0x1f, 0x8b, 0xff, 0xfe]).unwrap();
        assert!(matches!(
            s.file_read("main", "level.dat"),
            Err(Error::NotText(_))
        ));
    }
}
