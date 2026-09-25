import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { cn } from "@/lib/utils";

/**
 * Offline accounts have no skin to fetch, so the avatar is the name's
 * initial on a colour derived from the name (stable across launches).
 */
export function PlayerAvatar({ name, className }: { name: string; className?: string }) {
  let hash = 0;
  for (const ch of name) hash = (hash * 31 + ch.charCodeAt(0)) | 0;
  const hue = Math.abs(hash) % 360;
  return (
    <Avatar className={cn("size-7", className)}>
      <AvatarFallback
        className="text-xs font-semibold text-white"
        style={{ backgroundColor: `oklch(0.6 0.13 ${hue})` }}
      >
        {name.charAt(0).toUpperCase()}
      </AvatarFallback>
    </Avatar>
  );
}
