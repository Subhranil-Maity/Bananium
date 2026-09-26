//! Modrinth browsing and per-instance content management (mods, resource
//! packs, shader packs).
//!
//! Downloaded files go into the content-addressed store first and are then
//! materialized (reflink/hardlink) into the instance's folder, exactly like
//! libraries — the same mod in five instances costs its bytes once.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use bananium_instance::{ContentEntry, ContentKind, ContentStore, Loader, ModrinthStatus};
use bananium_modrinth::{
    Facet, FacetsBuilder, HashAlgorithm, SearchQuery, Sort, UpdateVersionFilesRequest, Version,
    VersionsFilter,
};
use bananium_net::DownloadSpec;
use bananium_store::BlobStore;

use super::tasks::TaskSpec;
use super::{blocking, Session};
use crate::command::SearchSort;
use crate::error::{Error, Result};
use crate::output::{
    CommandOutput, ContentUpdateInfo, GalleryItem, ModrinthHit, ModrinthProject, ModrinthVersion,
};
use crate::task::TaskKind;

/// Iris, the shader loader for Fabric. Every shader pack needs it, but
/// shader packs can't declare that as a Modrinth dependency themselves.
const IRIS: &str = "iris";

/// Guard against a pathological dependency graph pulling in half of Modrinth.
const MAX_RESOLVED: usize = 64;

/// Hashes per `version_files` request when identifying. Keeps each request
/// small enough to answer quickly (and to retry cheaply), and lets the task
/// report progress between chunks instead of sitting on one step.
const IDENTIFY_CHUNK: usize = 200;

/// Which version of a project to install. Automatic choices are always
/// **stable releases**; a beta/alpha is only installed when a user picks
/// that exact version ([`Pin::Exact`]) or an author pins it as a
/// dependency ([`Pin::Prefer`]).
pub(super) enum Pin {
    /// The newest stable version the instance can use.
    Newest,
    /// Exactly this version id — the user chose it, any channel.
    Exact(String),
    /// A dependency the author pinned to this version id: use it if the
    /// instance can, else fall back to [`Pin::Newest`].
    Prefer(String),
    /// A dependency the author left unpinned, required by a version
    /// published at this ISO-8601 time: the newest stable version released
    /// no later than that, i.e. what existed when the author tested it.
    /// Modrinth has no dependency version *ranges*, so this is how an
    /// unpinned requirement is kept from jumping to a later, breaking
    /// release — e.g. Iris 1.8.8 lists Sodium with no version, and needs
    /// the 0.6.x of its day, not today's 0.8.x (which breaks it).
    NotAfter(String),
}
/// What content can go into an instance is decided by these two facts.
pub(super) struct Target {
    pub(super) slug: String,
    pub(super) mc_version: String,
    pub(super) loader: Loader,
}

/// Modrinth "loaders" a version must list to be usable by `target` for
/// `kind`: Fabric for mods, Iris (or OptiFine-format packs, which Iris also
/// runs) for shaders, and the pseudo-loader `minecraft` for resource packs.
fn compatible_loaders(kind: ContentKind) -> Vec<String> {
    match kind {
        ContentKind::Mod => vec!["fabric".into()],
        ContentKind::Shader => vec!["iris".into(), "optifine".into()],
        ContentKind::ResourcePack => vec!["minecraft".into()],
    }
}

/// Shader packs are mostly independent of the Minecraft version (they
/// target the shader loader instead), so a version-exact match isn't
/// required for them; mods and resource packs must match.
fn version_bound(kind: ContentKind) -> bool {
    kind != ContentKind::Shader
}

fn is_compatible(version: &Version, kind: ContentKind, target: &Target) -> bool {
    let loaders = compatible_loaders(kind);
    version.loaders.iter().any(|l| loaders.contains(l))
        && (!version_bound(kind) || version.game_versions.contains(&target.mc_version))
}

/// The only channel automatic choices (install, dependencies, updates) use.
const STABLE: &str = "release";

/// Modrinth timestamps are uniform ISO-8601 UTC strings, so they order
/// correctly as plain strings.
fn published_not_after(v: &Version, cutoff: &str) -> bool {
    v.date_published.as_str() <= cutoff
}

/// Whether `v` declares `project` (optionally one specific version of it)
/// incompatible.
fn declares_incompatible(v: &Version, project: &str, project_version: &str) -> bool {
    v.dependencies.iter().any(|d| {
        d.dependency_type == "incompatible"
            && d.project_id.as_deref() == Some(project)
            && d.version_id
                .as_deref()
                .is_none_or(|id| id == project_version)
    })
}
impl Session {
    pub(super) fn content(&self) -> ContentStore {
        ContentStore::new(self.paths.clone())
    }

    pub(super) fn target(&self, slug: &str) -> Result<Target> {
        let cfg = self.instances().load(slug)?;
        Ok(Target {
            slug: slug.to_string(),
            mc_version: cfg.mc_version,
            loader: cfg.loader,
        })
    }

    /// Mods and shaders need a mod loader; a vanilla instance can only
    /// take resource packs.
    fn check_kind_allowed(&self, kind: ContentKind, target: &Target) -> Result<()> {
        if kind != ContentKind::ResourcePack && target.loader == Loader::Vanilla {
            return Err(Error::NeedsFabric(target.slug.clone()));
        }
        Ok(())
    }

    /// `Command::ModrinthSearch`.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn modrinth_search(
        &self,
        query: &str,
        kind: ContentKind,
        instance: Option<&str>,
        categories: &[String],
        sort: SearchSort,
        offset: u32,
        limit: u32,
    ) -> Result<CommandOutput> {
        let mut facets = FacetsBuilder::new().and(Facet::project_type(kind.modrinth_type()));
        let mut installed = HashSet::new();
        if let Some(slug) = instance {
            let target = self.target(slug)?;
            if version_bound(kind) {
                facets = facets.and(Facet::version(&target.mc_version));
            }
            if kind != ContentKind::ResourcePack {
                facets = facets.group(compatible_loaders(kind).into_iter().map(Facet::loader));
            }
            installed = self.content().installed_projects(slug)?;
        }
        for c in categories {
            facets = facets.and(Facet::category(c));
        }
        self.run_search(facets, query, sort, offset, limit, &installed)
            .await
    }

    /// Run a Modrinth search with prepared `facets`, marking hits whose
    /// project is in `installed`.
    pub(super) async fn run_search(
        &self,
        facets: FacetsBuilder,
        query: &str,
        sort: SearchSort,
        offset: u32,
        limit: u32,
        installed: &HashSet<String>,
    ) -> Result<CommandOutput> {
        let mut search = SearchQuery::new()
            .facets(facets.build())
            .index(match sort {
                SearchSort::Relevance => Sort::Relevance,
                SearchSort::Downloads => Sort::Downloads,
                SearchSort::Follows => Sort::Follows,
                SearchSort::Newest => Sort::Newest,
                SearchSort::Updated => Sort::Updated,
            })
            .offset(offset)
            .limit(if limit == 0 { 20 } else { limit.min(100) });
        if !query.trim().is_empty() {
            search = search.query(query.trim());
        }
        let started = std::time::Instant::now();
        let res = self.modrinth.search(&search).await?;
        // Just the query and paging (truncated): enough to follow what the
        // user was looking for in a pasted log.
        let shown: String = query.trim().chars().take(100).collect();
        tracing::info!(
            query = %shown,
            ?sort,
            offset,
            hits = res.hits.len(),
            total = res.total_hits,
            ms = started.elapsed().as_millis() as u64,
            "modrinth search"
        );
        let hits = res
            .hits
            .into_iter()
            .map(|h| ModrinthHit {
                installed: installed.contains(&h.project_id),
                project_id: h.project_id,
                slug: h.slug,
                title: h.title,
                description: h.description,
                author: h.author,
                icon_url: h.icon_url,
                downloads: h.downloads,
                follows: h.follows,
                categories: h.display_categories,
                date_modified: h.date_modified,
            })
            .collect();
        Ok(CommandOutput::ModrinthSearched {
            hits,
            offset: res.offset,
            total_hits: res.total_hits,
        })
    }

    /// `Command::ModrinthProject`.
    pub(super) async fn modrinth_project(&self, project: &str) -> Result<CommandOutput> {
        let started = std::time::Instant::now();
        let p = self.modrinth.project(project).await?;
        tracing::info!(
            project,
            ms = started.elapsed().as_millis() as u64,
            "opened modrinth project"
        );
        let mut gallery = p.gallery;
        gallery.sort_by_key(|g| (!g.featured, g.ordering));
        Ok(CommandOutput::ModrinthProjectShown {
            project: ModrinthProject {
                id: p.id,
                slug: p.slug,
                title: p.title,
                description: p.description,
                body: p.body,
                project_type: p.project_type,
                icon_url: p.icon_url,
                gallery: gallery
                    .into_iter()
                    .map(|g| GalleryItem {
                        url: g.url,
                        title: g.title,
                    })
                    .collect(),
                downloads: p.downloads,
                followers: p.followers,
                categories: p.categories,
                license: p.license.map(|l| l.id),
                updated: p.updated,
                game_versions: p.game_versions,
                loaders: p.loaders,
                source_url: p.source_url,
                issues_url: p.issues_url,
                wiki_url: p.wiki_url,
                discord_url: p.discord_url,
            },
        })
    }

    /// `Command::ModrinthVersions`.
    pub(super) async fn modrinth_versions(
        &self,
        project: &str,
        kind: ContentKind,
        instance: Option<&str>,
    ) -> Result<CommandOutput> {
        let target = instance.map(|s| self.target(s)).transpose()?;
        let versions = self
            .modrinth
            .project_versions(project, &VersionsFilter::default())
            .await?
            .into_iter()
            .map(|v| ModrinthVersion {
                compatible: target.as_ref().is_none_or(|t| is_compatible(&v, kind, t)),
                id: v.id,
                name: v.name,
                version_number: v.version_number,
                version_type: v.version_type,
                date_published: v.date_published,
                downloads: v.downloads,
                game_versions: v.game_versions,
                loaders: v.loaders,
            })
            .collect();
        Ok(CommandOutput::ModrinthVersionsListed { versions })
    }

    /// `Command::ContentList`.
    pub(super) fn content_list(&self, instance: &str) -> Result<CommandOutput> {
        Ok(CommandOutput::ContentListed {
            instance: instance.to_string(),
            entries: self.content().sync(instance)?,
        })
    }

    /// Every acceptable version of `project` for `target` under `pin`, most
    /// preferred first; the resolver walks this list when the first choice
    /// conflicts with something already installed. Only the Modrinth API is
    /// consulted: a version is acceptable when it lists the instance's
    /// Minecraft version and a usable loader.
    async fn candidates(
        &self,
        project: &str,
        pin: &Pin,
        kind: ContentKind,
        target: &Target,
    ) -> Result<Vec<Version>> {
        if let Pin::Exact(id) = pin {
            let v = self
                .modrinth
                .project_versions(project, &VersionsFilter::default())
                .await?
                .into_iter()
                .find(|v| &v.id == id)
                .ok_or_else(|| Error::VersionNotFound(id.clone()))?;
            return Ok(vec![v]);
        }

        let filter = VersionsFilter {
            loaders: Some(compatible_loaders(kind)),
            game_versions: version_bound(kind).then(|| vec![target.mc_version.clone()]),
            featured: None,
        };
        // Modrinth lists newest first.
        let usable = self.modrinth.project_versions(project, &filter).await?;
        let (stable, prerelease): (Vec<Version>, Vec<Version>) =
            usable.into_iter().partition(|v| v.version_type == STABLE);

        let mut ordered = Vec::new();
        match pin {
            Pin::Prefer(id) => {
                // The author's pin wins even if it's a beta: it's the one
                // version they declared this dependency works with.
                if let Some(v) = stable.iter().chain(&prerelease).find(|v| &v.id == id) {
                    ordered.push(v.clone());
                }
                ordered.extend(stable.iter().filter(|v| &v.id != id).cloned());
            }
            Pin::NotAfter(cutoff) => {
                let (before, after): (Vec<Version>, Vec<Version>) = stable
                    .into_iter()
                    .partition(|v| published_not_after(v, cutoff));
                ordered.extend(before);
                ordered.extend(after);
            }
            Pin::Newest | Pin::Exact(_) => ordered = stable,
        }

        if ordered.is_empty() {
            return Err(if prerelease.is_empty() {
                Error::NoCompatibleVersion {
                    project: project.to_string(),
                    mc_version: target.mc_version.clone(),
                }
            } else {
                Error::OnlyPrereleases {
                    project: project.to_string(),
                    mc_version: target.mc_version.clone(),
                }
            });
        }
        Ok(ordered)
    }

    /// The single best version of `project` for `target`, per `pin`.
    pub(super) async fn choose_version(
        &self,
        project: &str,
        pin: &Pin,
        kind: ContentKind,
        target: &Target,
    ) -> Result<Version> {
        let mut all = self.candidates(project, pin, kind, target).await?;
        Ok(all.swap_remove(0))
    }

    /// The Modrinth versions of the instance's installed, enabled content,
    /// looked up by file hash — needed for their "incompatible" lists.
    async fn installed_versions(&self, instance: &str) -> Result<Vec<(String, Version)>> {
        let entries: Vec<ContentEntry> = self
            .content()
            .sync(instance)?
            .into_iter()
            .filter(|e| e.enabled && e.project_id.is_some())
            .collect();
        let hashes: Vec<String> = entries.iter().filter_map(|e| e.sha1.clone()).collect();
        if hashes.is_empty() {
            return Ok(Vec::new());
        }
        let found = self
            .modrinth
            .version_files(&hashes, HashAlgorithm::Sha1)
            .await?;
        Ok(entries
            .into_iter()
            .filter_map(|e| {
                let v = found.get(e.sha1.as_deref()?)?.clone();
                Some((e.title, v))
            })
            .collect())
    }

    /// Display name of `instance` for task labels (its slug if it can't
    /// be read).
    pub(super) fn instance_name(&self, instance: &str) -> String {
        self.instances()
            .load(instance)
            .map(|cfg| cfg.name)
            .unwrap_or_else(|_| instance.to_string())
    }

    /// `Command::ContentInstall`, as one queued task.
    pub(super) async fn content_install_tracked(
        &self,
        instance: &str,
        project: String,
        version: Option<String>,
        kind: ContentKind,
    ) -> Result<CommandOutput> {
        let name = self.instance_name(instance);
        let label = format!("Installing {project} into {name}");
        let spec = TaskSpec::new(TaskKind::ContentInstall, label)
            .instance(instance)
            .project(project.clone());
        let ticket = self.enqueue_task("content", spec, || Ok(()))?;
        let task_id = ticket.task_id().to_string();
        self.presence
            .describe_task(&task_id, format!("Adding content to {name}"), None);
        let roots = vec![(project, version, kind)];
        let installed = self
            .tracked(ticket, self.content_install(&task_id, instance, roots))
            .await?;
        Ok(CommandOutput::ContentInstalled {
            instance: instance.to_string(),
            installed,
        })
    }

    /// Resolve `roots` plus their required dependencies (breadth-first,
    /// skipping anything already installed), download every file into the
    /// store, and place each into the instance. Roots are (re)installed
    /// even when present — that's how an update or a version change works.
    pub(super) async fn content_install(
        &self,
        task_id: &str,
        instance: &str,
        roots: Vec<(String, Option<String>, ContentKind)>,
    ) -> Result<Vec<ContentEntry>> {
        let target = self.target(instance)?;
        let store = self.content();
        self.phase_progress(task_id, "Resolving dependencies", 0, 1, &mut None);
        let installed = store.installed_projects(instance)?;
        let root_ids: HashSet<String> = roots.iter().map(|(p, _, _)| p.clone()).collect();

        let mut queue: VecDeque<(String, Pin, ContentKind, bool)> = VecDeque::new();
        for (project, version, kind) in roots {
            self.check_kind_allowed(kind, &target)?;
            if kind == ContentKind::Shader {
                queue.push_back((IRIS.to_string(), Pin::Newest, ContentKind::Mod, true));
            }
            let pin = version.map_or(Pin::Newest, Pin::Exact);
            queue.push_front((project, pin, kind, false));
        }

        // Everything that will be enabled afterwards, as (title, version):
        // installed content that isn't being replaced, plus each pick.
        // A candidate conflicts if it declares any of these incompatible,
        // or any of these declares it incompatible.
        let mut present: Vec<(String, Version)> = self
            .installed_versions(instance)
            .await?
            .into_iter()
            .filter(|(_, v)| !root_ids.contains(&v.project_id))
            .collect();

        let mut seen = HashSet::new();
        let mut picked: Vec<(Version, ContentKind, bool)> = Vec::new();
        while let Some((project, pin, kind, dependency)) = queue.pop_front() {
            if picked.len() >= MAX_RESOLVED {
                return Err(Error::TooManyDependencies(MAX_RESOLVED));
            }
            let options = self.candidates(&project, &pin, kind, &target).await?;
            let project_id = options[0].project_id.clone();
            if seen.contains(&project_id) || (dependency && installed.contains(&project_id)) {
                continue;
            }

            // Walk newest-preferred → older until one fits alongside
            // everything present. A project-wide incompatibility declared by
            // something installed can't be fixed by an older version, so
            // that fails straight away with the offending pair named.
            let mut chosen = None;
            let mut conflict = String::new();
            for v in options {
                if let Some((title, _)) = present
                    .iter()
                    .find(|(_, p)| declares_incompatible(p, &v.project_id, &v.id))
                {
                    conflict = format!("{title} is incompatible with {project}");
                    tracing::debug!("skipping {project} {}: {conflict}", v.version_number);
                    continue;
                }
                if let Some((title, _)) = present
                    .iter()
                    .find(|(_, p)| declares_incompatible(&v, &p.project_id, &p.id))
                {
                    conflict = format!(
                        "{project} {} is incompatible with {title}",
                        v.version_number
                    );
                    tracing::debug!("skipping: {conflict}");
                    continue;
                }
                chosen = Some(v);
                break;
            }
            let Some(v) = chosen else {
                tracing::warn!(instance, "refusing to install {project}: {conflict}");
                return Err(Error::Incompatible(conflict));
            };
            tracing::info!(
                instance,
                dependency,
                "picked {project} {} ({}) for {kind:?}",
                v.version_number,
                v.id
            );

            seen.insert(v.project_id.clone());
            for dep in &v.dependencies {
                if dep.dependency_type != "required" {
                    continue;
                }
                if let Some(pid) = &dep.project_id {
                    let pin = match &dep.version_id {
                        Some(id) => Pin::Prefer(id.clone()),
                        None => Pin::NotAfter(v.date_published.clone()),
                    };
                    queue.push_back((pid.clone(), pin, ContentKind::Mod, true));
                }
            }
            present.push((project.clone(), v.clone()));
            picked.push((v, kind, dependency));
        }

        // Every picked file's project (for its title and icon) in batched
        // lookups, not one request per file against the rate limit.
        let ids: Vec<String> = picked
            .iter()
            .map(|(v, _, _)| v.project_id.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let mut projects = HashMap::new();
        for chunk in ids.chunks(100) {
            for p in self.modrinth.projects(chunk).await? {
                projects.insert(p.id.clone(), p);
            }
        }
        let mut specs = Vec::new();
        let mut planned = Vec::new();
        for (version, kind, dependency) in picked {
            let file = version
                .files
                .iter()
                .find(|f| f.primary)
                .or(version.files.first())
                .ok_or_else(|| Error::NoFile(version.id.clone()))?;
            let sha1 = file
                .hashes
                .sha1
                .clone()
                .ok_or_else(|| Error::NoFile(version.id.clone()))?;
            let (title, icon_url) = match projects.get(&version.project_id) {
                Some(p) => (p.title.clone(), p.icon_url.clone()),
                None => (version.name.clone(), None),
            };
            specs.push(DownloadSpec {
                url: file.url.clone(),
                dest: self.paths.store_blob(&sha1),
                expected_sha1: Some(sha1.clone()),
                expected_size: Some(file.size),
                task_id: format!("{task_id}/{}", version.project_id),
                label: file.filename.clone(),
            });
            planned.push(ContentEntry {
                kind,
                filename: file.filename.clone(),
                enabled: true,
                title,
                project_id: Some(version.project_id.clone()),
                version_id: Some(version.id.clone()),
                version_number: Some(version.version_number.clone()),
                icon_url,
                sha1: Some(sha1),
                dependency,
                modrinth: ModrinthStatus::Identified,
            });
        }

        let label = match planned.first() {
            Some(first) if planned.len() > 1 => {
                format!("{} + {} dependencies", first.title, planned.len() - 1)
            }
            Some(first) => first.title.clone(),
            None => "content".to_string(),
        };
        self.download_tracked(task_id, &label, specs).await?;

        let blobs = BlobStore::new(self.paths.clone());
        for entry in &planned {
            let dest = store.file_path(instance, entry);
            remove_if_exists(&dest)?;
            let disabled = self
                .content()
                .dir(instance, entry.kind)
                .join(format!("{}.disabled", entry.filename));
            remove_if_exists(&disabled)?;
            blobs.materialize(entry.sha1.as_deref().unwrap_or_default(), &dest)?;
            store.upsert(instance, entry.clone())?;
        }
        Ok(planned)
    }

    /// `Command::ContentRemove`.
    pub(super) fn content_remove(
        &self,
        instance: &str,
        kind: ContentKind,
        filename: &str,
    ) -> Result<CommandOutput> {
        self.content().remove(instance, kind, filename)?;
        tracing::info!(instance, "removed {kind:?} {filename}");
        Ok(CommandOutput::ContentRemoved {
            instance: instance.to_string(),
            filename: filename.to_string(),
        })
    }

    /// `Command::ContentToggle`.
    pub(super) fn content_toggle(
        &self,
        instance: &str,
        kind: ContentKind,
        filename: &str,
        enabled: bool,
    ) -> Result<CommandOutput> {
        self.content()
            .set_enabled(instance, kind, filename, enabled)?;
        tracing::info!(instance, enabled, "toggled {kind:?} {filename}");
        Ok(CommandOutput::ContentToggled {
            instance: instance.to_string(),
            filename: filename.to_string(),
            enabled,
        })
    }

    /// `Command::ContentImport`: copy a local file in. It starts out
    /// unchecked; identifying it on Modrinth is the user's call (the
    /// instance page offers it whenever anything is left unchecked).
    pub(super) async fn content_import(
        &self,
        instance: &str,
        kind: ContentKind,
        path: &Path,
    ) -> Result<CommandOutput> {
        let target = self.target(instance)?;
        self.check_kind_allowed(kind, &target)?;
        let filename = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| Error::UnsupportedFile(path.display().to_string()))?;
        let wanted = match kind {
            ContentKind::Mod => ".jar",
            ContentKind::ResourcePack | ContentKind::Shader => ".zip",
        };
        if !filename.to_lowercase().ends_with(wanted) {
            return Err(Error::UnsupportedFile(filename));
        }
        let dir = self.content().dir(instance, kind);
        std::fs::create_dir_all(&dir)?;
        std::fs::copy(path, dir.join(&filename))?;
        tracing::info!(instance, "imported {kind:?} {filename}");
        let entry = self
            .content()
            .sync(instance)?
            .into_iter()
            .find(|e| e.kind == kind && e.filename == filename)
            .ok_or_else(|| Error::UnsupportedFile(filename.clone()))?;
        Ok(CommandOutput::ContentImported {
            instance: instance.to_string(),
            entry,
        })
    }

    /// `Command::ContentIdentify`, as one queued task. Manual on purpose:
    /// it used to run at the end of every modpack install, where a slow
    /// Modrinth made the whole install look stuck.
    pub(super) async fn content_identify(&self, instance: &str) -> Result<CommandOutput> {
        let name = self.instance_name(instance);
        let spec = TaskSpec::new(
            TaskKind::ContentIdentify,
            format!("Identifying mods in {name}"),
        )
        .instance(instance);
        let ticket = self.enqueue_task("identify", spec, || Ok(()))?;
        let task_id = ticket.task_id().to_string();
        let (identified, not_found) = self
            .tracked(ticket, self.identify(&task_id, instance))
            .await?;
        Ok(CommandOutput::ContentIdentified {
            instance: instance.to_string(),
            identified,
            not_found,
        })
    }

    /// Look every [`ModrinthStatus::Unchecked`] file up on Modrinth by
    /// SHA-1: a match records its project/version/title/icon and becomes
    /// `Identified`; no match becomes `NotFound` (logged by name, so the
    /// user can see which files those are) and is never asked about again.
    /// Returns `(identified, not_found)`. All results land in one lockfile
    /// write, at the end.
    pub(super) async fn identify(&self, task_id: &str, instance: &str) -> Result<(u32, u32)> {
        let started = std::time::Instant::now();
        const STEP: &str = "Identifying mods";
        self.phase_progress(task_id, STEP, 0, 1, &mut None);
        // Syncing hashes every file it hasn't seen yet — a whole modpack's
        // worth on the first run — so it runs off the async runtime.
        let entries = {
            let store = self.content();
            let slug = instance.to_string();
            blocking(move || Ok(store.sync(&slug)?)).await?
        };
        let unchecked: Vec<&ContentEntry> = entries
            .iter()
            .filter(|e| e.modrinth == ModrinthStatus::Unchecked && e.sha1.is_some())
            .collect();
        let already_known = entries.len() - unchecked.len();
        if unchecked.is_empty() {
            tracing::info!(instance, "identify: nothing left to check");
            self.phase_progress(task_id, STEP, 1, 1, &mut None);
            return Ok((0, 0));
        }

        let hashes: Vec<String> = unchecked.iter().filter_map(|e| e.sha1.clone()).collect();
        let mut found = HashMap::new();
        let mut requests = 0;
        let mut last = None;
        for (i, chunk) in hashes.chunks(IDENTIFY_CHUNK).enumerate() {
            self.phase_progress(
                task_id,
                &format!("{STEP} ({}/{})", i * IDENTIFY_CHUNK, hashes.len()),
                i * IDENTIFY_CHUNK,
                hashes.len(),
                &mut last,
            );
            found.extend(
                self.modrinth
                    .version_files(chunk, HashAlgorithm::Sha1)
                    .await?,
            );
            requests += 1;
        }
        // One batched lookup for every matched project (a modpack can match
        // hundreds), rather than a request each against the rate limit.
        let ids: Vec<String> = found
            .values()
            .map(|v| v.project_id.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let mut projects = HashMap::new();
        for chunk in ids.chunks(100) {
            for p in self.modrinth.projects(chunk).await? {
                projects.insert(p.id.clone(), p);
            }
            requests += 1;
        }
        self.phase_progress(task_id, "Saving", hashes.len(), hashes.len(), &mut None);

        let mut identified = 0;
        let mut not_found = 0;
        let mut updates = HashMap::new();
        for entry in &unchecked {
            let key = (entry.kind, entry.filename.clone());
            let matched = entry
                .sha1
                .as_ref()
                .and_then(|h| found.get(h))
                .and_then(|v| Some((v, projects.get(&v.project_id)?)));
            match matched {
                Some((version, project)) => {
                    identified += 1;
                    updates.insert(key, Some((version.clone(), project.clone())));
                }
                None => {
                    not_found += 1;
                    tracing::info!(
                        instance,
                        "identify: {}/{} (sha1 {}) not found on Modrinth",
                        entry.kind.dir_name(),
                        entry.filename,
                        entry.sha1.as_deref().map_or("-", |h| &h[..h.len().min(12)])
                    );
                    updates.insert(key, None);
                }
            }
        }
        let store = self.content();
        let slug = instance.to_string();
        blocking(move || {
            Ok(store.update_all(&slug, |e| {
                let Some(result) = updates.get(&(e.kind, e.filename.clone())) else {
                    return;
                };
                match result {
                    Some((version, project)) => {
                        e.title = project.title.clone();
                        e.icon_url = project.icon_url.clone();
                        e.project_id = Some(version.project_id.clone());
                        e.version_id = Some(version.id.clone());
                        e.version_number = Some(version.version_number.clone());
                        e.modrinth = ModrinthStatus::Identified;
                    }
                    None => e.modrinth = ModrinthStatus::NotFound,
                }
            })?)
        })
        .await?;
        tracing::info!(
            instance,
            "identify: {identified} identified, {not_found} not on Modrinth, {already_known} already known, in {}ms ({requests} requests)",
            started.elapsed().as_millis()
        );
        Ok((identified, not_found))
    }

    /// Which installed Modrinth content has an update, and to which version.
    ///
    /// Stable releases only, and never past what installed dependents
    /// allow. From the Modrinth API alone: if an installed mod pins a
    /// dependency's version, that dependency isn't updated; if it requires
    /// it unpinned, the dependency is updated at most to the newest stable
    /// version released no later than that mod (see [`Pin::NotAfter`]) —
    /// otherwise updating Sodium would pull 0.8.x under Iris 1.8.8 and
    /// break it. A candidate that declares, or is declared, incompatible
    /// with anything else installed is never offered.
    async fn find_updates(&self, instance: &str) -> Result<Vec<ContentUpdateInfo>> {
        let target = self.target(instance)?;
        let entries = self.content().sync(instance)?;
        let installed = self.installed_versions(instance).await?;

        // Caps imposed by installed dependents, per dependency project.
        let mut pinned: HashSet<String> = HashSet::new();
        let mut cutoff: HashMap<String, String> = HashMap::new();
        for (_, v) in &installed {
            for d in v
                .dependencies
                .iter()
                .filter(|d| d.dependency_type == "required")
            {
                let Some(pid) = &d.project_id else { continue };
                if d.version_id.is_some() {
                    pinned.insert(pid.clone());
                } else {
                    let c = cutoff
                        .entry(pid.clone())
                        .or_insert_with(|| v.date_published.clone());
                    if v.date_published < *c {
                        *c = v.date_published.clone();
                    }
                }
            }
        }
        let current: HashMap<&str, &Version> = installed
            .iter()
            .map(|(_, v)| (v.project_id.as_str(), v))
            .collect();
        let conflicts = |candidate: &Version| {
            installed.iter().any(|(_, p)| {
                p.project_id != candidate.project_id
                    && (declares_incompatible(p, &candidate.project_id, &candidate.id)
                        || declares_incompatible(candidate, &p.project_id, &p.id))
            })
        };

        let mut updates = Vec::new();
        let mut offer = |entry: &ContentEntry, kind: ContentKind, newest: &Version| {
            if entry.version_id.as_deref() != Some(newest.id.as_str()) && !conflicts(newest) {
                updates.push(ContentUpdateInfo {
                    project_id: newest.project_id.clone(),
                    kind,
                    filename: entry.filename.clone(),
                    title: entry.title.clone(),
                    current_version: entry.version_number.clone(),
                    new_version_id: newest.id.clone(),
                    new_version_number: newest.version_number.clone(),
                });
            }
        };

        for kind in ContentKind::ALL {
            let mut free: HashMap<&str, &ContentEntry> = HashMap::new();
            for e in entries.iter().filter(|e| e.kind == kind && e.enabled) {
                let (Some(pid), Some(hash)) = (e.project_id.as_deref(), e.sha1.as_deref()) else {
                    continue;
                };
                if pinned.contains(pid) {
                    continue;
                }
                if let Some(c) = cutoff.get(pid) {
                    // Capped: newest stable no later than the dependent.
                    let options = self
                        .candidates(pid, &Pin::NotAfter(c.clone()), kind, &target)
                        .await?;
                    let newer_than_current = |v: &Version| {
                        current
                            .get(pid)
                            .is_none_or(|cur| v.date_published > cur.date_published)
                    };
                    if let Some(v) = options
                        .iter()
                        .find(|v| published_not_after(v, c))
                        .filter(|v| newer_than_current(v))
                    {
                        offer(e, kind, v);
                    }
                    continue;
                }
                free.insert(hash, e);
            }
            if free.is_empty() {
                continue;
            }
            let game_versions = if version_bound(kind) {
                vec![target.mc_version.clone()]
            } else {
                Vec::new()
            };
            let req = UpdateVersionFilesRequest::new(
                free.keys().map(|h| h.to_string()).collect(),
                HashAlgorithm::Sha1,
                compatible_loaders(kind),
                game_versions,
            )
            .version_types(vec![STABLE.to_string()]);
            let latest = self.modrinth.update_version_files(&req).await?;
            for (hash, entry) in free {
                if let Some(newest) = latest.get(hash) {
                    offer(entry, kind, newest);
                }
            }
        }
        updates.sort_by_key(|u| u.title.to_lowercase());
        Ok(updates)
    }

    /// `Command::ContentCheckUpdates`.
    pub(super) async fn content_check_updates(&self, instance: &str) -> Result<CommandOutput> {
        Ok(CommandOutput::ContentUpdatesFound {
            instance: instance.to_string(),
            updates: self.find_updates(instance).await?,
        })
    }

    /// `Command::ContentUpdate`: install exactly the versions
    /// [`Session::find_updates`] offers for `projects` (so an update is
    /// always what the check showed); projects with no update are skipped.
    pub(super) async fn content_update(
        &self,
        instance: &str,
        projects: &[String],
    ) -> Result<CommandOutput> {
        let name = self.instance_name(instance);
        let label = match projects.len() {
            1 => format!("Updating 1 item in {name}"),
            n => format!("Updating {n} items in {name}"),
        };
        let spec = TaskSpec::new(TaskKind::ContentUpdate, label).instance(instance);
        let ticket = self.enqueue_task("update", spec, || Ok(()))?;
        let task_id = ticket.task_id().to_string();
        self.presence
            .describe_task(&task_id, format!("Updating content in {name}"), None);
        let installed = self
            .tracked(ticket, async {
                let wanted: HashSet<&str> = projects.iter().map(String::as_str).collect();
                let roots: Vec<(String, Option<String>, ContentKind)> = self
                    .find_updates(instance)
                    .await?
                    .into_iter()
                    .filter(|u| wanted.contains(u.project_id.as_str()))
                    .map(|u| (u.project_id, Some(u.new_version_id), u.kind))
                    .collect();
                if roots.is_empty() {
                    return Ok(Vec::new());
                }
                self.content_install(&task_id, instance, roots).await
            })
            .await?;
        Ok(CommandOutput::ContentInstalled {
            instance: instance.to_string(),
            installed,
        })
    }
}

fn remove_if_exists(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(id: &str, channel: &str, loaders: &[&str], games: &[&str]) -> Version {
        serde_json::from_value(serde_json::json!({
            "id": id, "project_id": "p", "author_id": "a",
            "date_published": "2026-01-01T00:00:00Z", "downloads": 0,
            "version_type": channel, "loaders": loaders, "game_versions": games,
        }))
        .unwrap()
    }

    fn fabric_target() -> Target {
        Target {
            slug: "t".into(),
            mc_version: "1.21.1".into(),
            loader: Loader::Fabric,
        }
    }

    #[test]
    fn publish_cutoff_compares_iso_timestamps() {
        // Iris 1.8.8 (unpinned Sodium dep) must not pull a Sodium released
        // after it — that's how the incompatible 0.8.x got picked.
        let mut sodium_06 = version("s06", "release", &["fabric"], &["1.21.1"]);
        sodium_06.date_published = "2024-12-01T10:00:00.000000Z".into();
        let mut sodium_08 = version("s08", "release", &["fabric"], &["1.21.1"]);
        sodium_08.date_published = "2026-03-01T10:00:00.000000Z".into();
        let iris_188 = "2025-01-15T12:00:00.000000Z";
        assert!(published_not_after(&sodium_06, iris_188));
        assert!(!published_not_after(&sodium_08, iris_188));
    }

    #[test]
    fn incompatibility_can_target_a_project_or_one_version() {
        let mut v = version("a1", "release", &["fabric"], &["1.21.1"]);
        v.dependencies = serde_json::from_value(serde_json::json!([
            {"project_id": "whole", "dependency_type": "incompatible"},
            {"project_id": "one", "version_id": "bad", "dependency_type": "incompatible"},
            {"project_id": "req", "dependency_type": "required"},
        ]))
        .unwrap();
        assert!(declares_incompatible(&v, "whole", "any"));
        assert!(declares_incompatible(&v, "one", "bad"));
        assert!(!declares_incompatible(&v, "one", "good"));
        assert!(!declares_incompatible(&v, "req", "any"));
    }

    #[test]
    fn compatibility_follows_kind_rules() {
        let t = fabric_target();
        let fabric_mod = version("m", "release", &["fabric"], &["1.21.1"]);
        assert!(is_compatible(&fabric_mod, ContentKind::Mod, &t));
        let wrong_mc = version("m", "release", &["fabric"], &["1.20.1"]);
        assert!(!is_compatible(&wrong_mc, ContentKind::Mod, &t));
        let forge_mod = version("m", "release", &["forge"], &["1.21.1"]);
        assert!(!is_compatible(&forge_mod, ContentKind::Mod, &t));
        // Shader packs follow the shader loader, not the Minecraft version.
        let shader = version("s", "release", &["optifine"], &["1.20.1"]);
        assert!(is_compatible(&shader, ContentKind::Shader, &t));
        let pack = version("r", "release", &["minecraft"], &["1.21.1"]);
        assert!(is_compatible(&pack, ContentKind::ResourcePack, &t));
    }
}
