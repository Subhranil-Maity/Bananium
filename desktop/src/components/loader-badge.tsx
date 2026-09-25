import type { InstanceSummary } from "@/bindings/InstanceSummary";
import { Badge } from "@/components/ui/badge";

/** "Fabric 0.16.9" for modded instances; nothing for vanilla. */
export function LoaderBadge({ instance }: { instance: InstanceSummary }) {
  if (instance.loader !== "fabric") return null;
  return (
    <Badge variant="secondary" title={`Fabric loader ${instance.loader_version ?? ""}`}>
      Fabric {instance.loader_version}
    </Badge>
  );
}
