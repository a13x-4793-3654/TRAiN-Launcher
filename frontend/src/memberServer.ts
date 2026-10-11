import type { CSSProperties } from "react";
import { makeStyles, tokens } from "@fluentui/react-components";

export const useWorkspaceStyles = makeStyles({
  surface: {
    // Fluent Card draws its outline on ::after, not on the root border.
    "::after": {
      borderTopColor: "var(--train-workspace-border)",
      borderRightColor: "var(--train-workspace-border)",
      borderBottomColor: "var(--train-workspace-border)",
      borderLeftColor: "var(--train-workspace-border)",
    },
    "--train-workspace-border": tokens.colorNeutralStroke1,
  },
});

export interface MemberServer {
  id: string;
  name: string;
  environment?: string | null;
  parent_server_id?: string | null;
  expires_at?: string | null;
  style?: {
    badge?: string | null;
    palette?: {
      background?: string | null;
      foreground?: string | null;
      border?: string | null;
    } | null;
  } | null;
}

function isHexColor(value: unknown): value is string {
  return typeof value === "string" && value.length === 7 && /^#[0-9a-fA-F]{6}$/.test(value);
}

export function workspaceBadge(server: MemberServer): string | undefined {
  if (server.environment !== "workspace") {
    return undefined;
  }
  const badge = server.style?.badge;
  if (badge == null) {
    return "検証";
  }
  if (typeof badge === "string" && badge.trim().length > 0 &&
      badge.length <= 32 && !/[\u0000-\u001f\u007f]/.test(badge)) {
    return badge;
  }
  console.warn("Ignoring invalid Workspace badge for server", server.id);
  return "検証";
}

export function workspaceSurfaceStyle(server: MemberServer):
  (CSSProperties & { "--train-workspace-border": string }) | undefined {
  if (server.environment !== "workspace" || server.style?.palette == null) {
    return undefined;
  }
  const { background, foreground, border } = server.style.palette;
  // Apply the palette atomically: partial colors can make text unreadable.
  if (!isHexColor(background) || !isHexColor(foreground) || !isHexColor(border)) {
    console.warn("Ignoring invalid Workspace palette for server", server.id);
    return undefined;
  }
  return {
    backgroundColor: background,
    color: foreground,
    borderColor: border,
    "--train-workspace-border": border,
  };
}
