import { useQuery } from "@tanstack/react-query";
import { getVersion } from "@tauri-apps/api/app";
import { Globe } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";

import { BrandMark } from "@/components/brand-mark";
import { Page, PageHeader, Section } from "@/components/page";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";

const REPO_URL = "https://github.com/Subhranil-Maity/Bananium";
const AUTHOR_NAME = "Subhranil Maity";
const AUTHOR_HANDLE = "Subhranil-Maity";
const AUTHOR_URL = `https://github.com/${AUTHOR_HANDLE}`;
const BLOG_URL = "https://subhranil-maity.github.io/";

/** GitHub's mark; lucide dropped brand icons. */
function GitHubIcon() {
  return (
    <svg viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z" />
    </svg>
  );
}

export function AboutPage() {
  const { data: version } = useQuery({ queryKey: ["app-version"], queryFn: getVersion, staleTime: Infinity });

  return (
    <Page className="max-w-4xl">
      <PageHeader title="About" />
      <div className="space-y-4">
        <Section title="Project">
          <div className="flex items-start gap-4">
            <BrandMark className="size-14 shrink-0" />
            <div className="min-w-0 flex-1 space-y-1">
              <div className="flex items-center gap-2">
                <h2 className="text-lg font-semibold tracking-tight">Bananium</h2>
                {version && <Badge variant="secondary">v{version}</Badge>}
              </div>
              <p className="text-sm text-muted-foreground">
                A fast, lightweight Minecraft launcher. Instances, Fabric, Modrinth mods, shaders,
                resource packs, and modpacks, with a Rust core that stays out of your RAM's way.
              </p>
              <p className="text-xs text-muted-foreground">Released under the MIT License.</p>
            </div>
          </div>
          <Button variant="outline" size="sm" onClick={() => void openUrl(REPO_URL)}>
            <GitHubIcon /> Subhranil-Maity/Bananium
          </Button>
        </Section>

        <Section title="Author">
          <div className="space-y-1">
            <p className="text-sm font-medium">{AUTHOR_NAME}</p>
            <p className="text-sm text-muted-foreground">Creator and maintainer of Bananium.</p>
          </div>
          <div className="flex flex-wrap gap-2">
            <Button variant="outline" size="sm" onClick={() => void openUrl(AUTHOR_URL)}>
              <GitHubIcon /> @{AUTHOR_HANDLE}
            </Button>
            <Button variant="outline" size="sm" onClick={() => void openUrl(BLOG_URL)}>
              <Globe /> subhranil-maity.github.io
            </Button>
          </div>
        </Section>
      </div>
    </Page>
  );
}
