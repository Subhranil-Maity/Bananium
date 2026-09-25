import { Loader2, Play, Square } from "lucide-react";

import type { InstanceSummary } from "@/bindings/InstanceSummary";
import { Button } from "@/components/ui/button";
import { useKill, useLaunch } from "@/hooks/use-instances";
import { cn } from "@/lib/utils";

/** Play when stopped, Stop when running. */
export function PlayButton({ instance, className }: { instance: InstanceSummary; className?: string }) {
  const launch = useLaunch();
  const kill = useKill();

  if (instance.running) {
    return (
      <Button
        variant="destructive"
        className={cn(className)}
        disabled={kill.isPending}
        onClick={(e) => {
          e.stopPropagation();
          kill.mutate(instance.slug);
        }}
      >
        {kill.isPending ? <Loader2 className="animate-spin" /> : <Square />}
        Stop
      </Button>
    );
  }
  return (
    <Button
      className={cn(className)}
      disabled={launch.isPending}
      onClick={(e) => {
        e.stopPropagation();
        launch.mutate(instance.slug);
      }}
    >
      {launch.isPending ? <Loader2 className="animate-spin" /> : <Play />}
      Play
    </Button>
  );
}
