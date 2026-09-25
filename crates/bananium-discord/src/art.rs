//! Bananium's own presence images, served straight from the repository.
//!
//! Discord accepts an `https://` URL anywhere it accepts an uploaded art
//! asset key, so nothing has to be uploaded to the Discord Developer Portal:
//! the PNGs live in `assets/discord/` and are linked through GitHub's raw
//! file host. Modpack and project icons don't need this — Modrinth's CDN
//! URLs are used as-is.

/// Where `assets/discord/` is served from.
pub const ART_BASE_URL: &str =
    "https://raw.githubusercontent.com/Subhranil-Maity/Bananium/main/assets/discord";

/// One image in `assets/discord/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Art {
    /// The pixel banana logo.
    Bananium,
    /// A grass block, standing for Minecraft itself / vanilla.
    Minecraft,
    /// The Fabric mod loader logo.
    Fabric,
    /// The Modrinth logo, for browsing and Modrinth projects.
    Modrinth,
    /// A download arrow, for installs and downloads in progress.
    Download,
    /// A coffee cup, for Java runtime downloads.
    Java,
}

impl Art {
    /// Every image, e.g. for a test that each one exists on disk.
    pub const ALL: [Art; 6] = [
        Art::Bananium,
        Art::Minecraft,
        Art::Fabric,
        Art::Modrinth,
        Art::Download,
        Art::Java,
    ];

    /// The file name in `assets/discord/`.
    pub fn file_name(self) -> &'static str {
        match self {
            Art::Bananium => "bananium.png",
            Art::Minecraft => "minecraft.png",
            Art::Fabric => "fabric.png",
            Art::Modrinth => "modrinth.png",
            Art::Download => "download.png",
            Art::Java => "java.png",
        }
    }

    /// The public URL Discord fetches the image from.
    pub fn url(self) -> String {
        format!("{ART_BASE_URL}/{}", self.file_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_art_file_is_in_the_repo() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/discord");
        if !dir.is_dir() {
            // Built outside the repository (e.g. a vendored copy): nothing to check.
            return;
        }
        for art in Art::ALL {
            assert!(
                dir.join(art.file_name()).is_file(),
                "missing assets/discord/{}",
                art.file_name()
            );
        }
    }

    #[test]
    fn urls_are_https_and_short_enough_for_discord() {
        for art in Art::ALL {
            let url = art.url();
            assert!(url.starts_with("https://"));
            assert!(url.len() <= 256, "{url}");
        }
    }
}
