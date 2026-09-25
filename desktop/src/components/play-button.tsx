import { Loader2, Play, Square } from "lucide-react";

import type { InstanceSummary } from "@/bindings/InstanceSummary";
import { Button } from "@/components/ui/button";
import { useKill, useLaunch } from "@/hooks/use-instances";
import { cn } from "@/lib/utils";

/** Play when stopped, Stop when running. `iconOnly` for dense rows. */
export function PlayButton({
  instance,
  className,
  size = "default",
  iconOnly = false,
}: {
  instance: InstanceSummary;
  className?: string;
  size?: "sm" | "default" | "lg";
  iconOnly?: boolean;
}) {
  const launch = useLaunch();
  const kill = useKill();
  const buttonSize = iconOnly ? (size === "sm" ? "icon-sm" : size === "lg" ? "icon-lg" : "icon") : size;

  if (instance.running) {
    return (
      <Button
        variant="outline"
        size={buttonSize}
        title="Stop the game"
        className={cn(
          "border-destructive/40 text-destructive hover:bg-destructive/10 hover:text-destructive dark:border-destructive/40 dark:hover:bg-destructive/15",
          className,
        )}
        disabled={kill.isPending}
        onClick={(e) => {
          e.stopPropagation();
          kill.mutate(instance.slug);
        }}
      >
        {kill.isPending ? <Loader2 className="animate-spin" /> : <Square className="fill-current" />}
        {!iconOnly && "Stop"}
      </Button>
    );
  }
  return (
    <Button
      size={buttonSize}
      title={`Play ${instance.name}`}
      className={cn("font-semibold", className)}
      disabled={launch.isPending}
      onClick={(e) => {
        e.stopPropagation();
        launch.mutate(instance.slug);
      }}
    >
      {launch.isPending ? <Loader2 className="animate-spin" /> : <Play className="fill-current" />}
      {!iconOnly && "Play"}
    </Button>
  );
}
