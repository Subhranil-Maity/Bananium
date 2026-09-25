import { useState } from "react";
import { useNavigate } from "react-router";
import { Plus } from "lucide-react";

import type { InstanceSummary } from "@/bindings/InstanceSummary";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { LoaderBadge } from "@/components/loader-badge";
import { NewInstanceDialog } from "@/components/new-instance-dialog";
import { PlayButton } from "@/components/play-button";
import { useInstances } from "@/hooks/use-instances";
import { errorMessage } from "@/lib/api";

function InstanceCard({ instance }: { instance: InstanceSummary }) {
  const navigate = useNavigate();
  return (
    <Card
      className="cursor-pointer gap-4 transition-colors hover:border-primary"
      onClick={() => navigate(`/instance/${instance.slug}`)}
    >
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <span className="truncate">{instance.name}</span>
          {instance.running && <Badge className="bg-green-600 text-white">Running</Badge>}
        </CardTitle>
        <CardDescription className="flex items-center gap-2">
          Minecraft {instance.mc_version}
          {instance.ram_mb ? ` · ${instance.ram_mb} MB` : ""}
          <LoaderBadge instance={instance} />
        </CardDescription>
      </CardHeader>
      <CardContent>
        <PlayButton instance={instance} className="w-full" />
      </CardContent>
    </Card>
  );
}

/** Every installed instance, with play/stop and a "New instance" entry point. */
export function LibraryPage() {
  const { data: instances, isLoading, error } = useInstances();
  const [creating, setCreating] = useState(false);

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold">Library</h1>
        <Button onClick={() => setCreating(true)}>
          <Plus /> New instance
        </Button>
      </div>

      {error && <p className="text-sm text-destructive">{errorMessage(error)}</p>}

      <div className="grid grid-cols-[repeat(auto-fill,minmax(260px,1fr))] gap-4">
        {isLoading && [0, 1, 2].map((i) => <Skeleton key={i} className="h-36" />)}
        {instances?.map((i) => <InstanceCard key={i.slug} instance={i} />)}
      </div>
      {instances?.length === 0 && (
        <div className="rounded-lg border border-dashed p-12 text-center text-muted-foreground">
          No instances yet. Create one to get started.
        </div>
      )}

      <NewInstanceDialog open={creating} onOpenChange={setCreating} />
    </div>
  );
}
